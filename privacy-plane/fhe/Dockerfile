# openfhe's PyPI wheel is a "py3-none-any" package that in practice only
# works against Ubuntu 24.04's system libraries (it bundles a compiled .so,
# not source) — see docs/architecture/ARCHITECTURE_DECISIONS.md's Phase 5
# FHE integration note. python:slim (Debian) doesn't satisfy that.
FROM ubuntu:24.04

RUN apt-get update && \
    apt-get install -y --no-install-recommends python3 python3-pip libgomp1 && \
    rm -rf /var/lib/apt/lists/*

# Ubuntu 24.04's system pip refuses unmanaged global installs (PEP 668).
# This container's only job is running this one service, so there's no
# competing environment to protect.
RUN pip install --no-cache-dir --break-system-packages \
    openfhe fastapi "uvicorn[standard]"

WORKDIR /app
COPY app.py .

EXPOSE 8090
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8090"]
