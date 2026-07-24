import base64
import os
import tempfile
from typing import List

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from openfhe import (
    BINARY,
    CCParamsBFVRNS,
    DeserializeCiphertext,
    GenCryptoContext,
    PKESchemeFeature,
    SerializeToFile,
)

app = FastAPI(title="ZenFabrique FHE service")

# Comfortably above any realistic *summed* msPlayed total for this baseline
# demo (individual songs run a few hundred thousand ms; this leaves headroom
# for summing many of them). BFV arithmetic is modular — a sum that overflows
# this wraps silently rather than erroring, so this bound is load-bearing,
# not decorative. Raising it would mean regenerating parameters/keys, which
# is why it's a startup-time constant rather than per-request.
PLAINTEXT_MODULUS = 100_000_000

# A single in-process keypair for the whole service's lifetime — a baseline
# privacy demo (Phase 5), not a multi-tenant key-management system. The
# secret key never leaves this process; only ciphertexts and the final
# decrypted aggregate cross the HTTP boundary (see /aggregate below).
_parameters = CCParamsBFVRNS()
_parameters.SetPlaintextModulus(PLAINTEXT_MODULUS)
_parameters.SetMultiplicativeDepth(0)  # only EvalAdd is used, never EvalMult

crypto_context = GenCryptoContext(_parameters)
crypto_context.Enable(PKESchemeFeature.PKE)
crypto_context.Enable(PKESchemeFeature.KEYSWITCH)
crypto_context.Enable(PKESchemeFeature.LEVELEDSHE)

key_pair = crypto_context.KeyGen()


# OpenFHE's Python bindings only serialize to/from a filesystem path (no
# in-memory buffer API), so ciphertexts are round-tripped through a temp
# file to cross the HTTP boundary as a base64 string.
def _serialize_ciphertext(ciphertext) -> str:
    with tempfile.NamedTemporaryFile(delete=False) as f:
        path = f.name
    try:
        if not SerializeToFile(path, ciphertext, BINARY):
            raise RuntimeError("OpenFHE failed to serialize ciphertext")
        with open(path, "rb") as f:
            return base64.b64encode(f.read()).decode("ascii")
    finally:
        os.unlink(path)


def _deserialize_ciphertext(encoded: str):
    raw = base64.b64decode(encoded)
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(raw)
        path = f.name
    try:
        ciphertext, ok = DeserializeCiphertext(path, BINARY)
        if not ok:
            raise RuntimeError("OpenFHE failed to deserialize ciphertext")
        return ciphertext
    finally:
        os.unlink(path)


class EncryptRequest(BaseModel):
    value: int


class EncryptResponse(BaseModel):
    ciphertext: str


class AggregateRequest(BaseModel):
    ciphertexts: List[str]


class AggregateResponse(BaseModel):
    sum: int


@app.get("/health")
def health():
    return {"status": "ok"}


@app.post("/encrypt", response_model=EncryptResponse)
def encrypt(req: EncryptRequest):
    if req.value < 0 or req.value >= PLAINTEXT_MODULUS:
        raise HTTPException(
            status_code=400,
            detail=f"value must be in [0, {PLAINTEXT_MODULUS})",
        )
    plaintext = crypto_context.MakeCoefPackedPlaintext([req.value])
    ciphertext = crypto_context.Encrypt(key_pair.publicKey, plaintext)
    return EncryptResponse(ciphertext=_serialize_ciphertext(ciphertext))


@app.post("/aggregate", response_model=AggregateResponse)
def aggregate(req: AggregateRequest):
    if not req.ciphertexts:
        raise HTTPException(status_code=400, detail="ciphertexts must be non-empty")

    ciphertexts = [_deserialize_ciphertext(c) for c in req.ciphertexts]

    # Homomorphic addition — every input above stays ciphertext the whole
    # time. Only the final aggregate is decrypted, below.
    total = ciphertexts[0]
    for ct in ciphertexts[1:]:
        total = crypto_context.EvalAdd(total, ct)

    result_plaintext = crypto_context.Decrypt(total, key_pair.secretKey)
    result_plaintext.SetLength(1)
    value = result_plaintext.GetCoefPackedValue()[0]
    return AggregateResponse(sum=int(value))
