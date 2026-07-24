package zenfabrique.schema_mutation

# Zero-Trust gate on the self-healing shim: an alias repair is a schema
# mutation (it changes how every future event with that raw key is
# interpreted), so it goes through policy instead of applying unconditionally
# just because the fuzzy-matcher found a unique candidate.
#
# input:
#   source:  the event's transport-level origin (filename stem or AMQP tag)
#   field:   canonical field being repaired, e.g. "userId"
#   raw_key: the actual JSON key the matcher resolved it to, e.g. "user_id"

default allow = false

# identity-bearing fields get stricter handling than incidental ones —
# widening how "userId" is recognized has PII-adjacent blast radius, so it's
# gated by source trust; a typo'd field like "trackId" doesn't carry the same
# risk and can heal itself for any source.
protected_fields := {"userId"}

trusted_sources := {"partner-feed", "internal-batch"}

allow if {
	not protected_fields[input.field]
}

allow if {
	protected_fields[input.field]
	trusted_sources[input.source]
}
