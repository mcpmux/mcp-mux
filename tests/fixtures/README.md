# CF Access test fixtures

`cf_access_test_private.pem` and `cf_access_test_pubkey.pem` are **test-only** RSA keys
generated with `openssl genrsa`. They are compiled into binaries only when the
`mcpmux-gateway/test-utils` feature is enabled (integration tests). Never use these keys
outside the test suite.
