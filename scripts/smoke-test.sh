#!/usr/bin/env sh
set -e
BASE=http://127.0.0.1:8000

# 1. Fetch login page (sets CSRF cookie)
curl -c /tmp/rustio.jar -b /tmp/rustio.jar -s -o /dev/null $BASE/admin/login

# 2. Extract CSRF token from the cookie jar
CSRF=$(awk '$6=="rustio_csrf" {print $7}' /tmp/rustio.jar)
test -n "$CSRF" || { echo "no CSRF cookie"; exit 1; }

# 3. Log in
curl -c /tmp/rustio.jar -b /tmp/rustio.jar -s -o /dev/null \
  -X POST $BASE/admin/login \
  -d "_csrf=$CSRF&email=admin@example.com&password=admin"

# 4. Verify we're authenticated
code=$(curl -b /tmp/rustio.jar -s -o /dev/null -w '%{http_code}' $BASE/admin)
test "$code" = "200" || { echo "dashboard returned $code"; exit 1; }

# 5. Create a post via the admin
curl -b /tmp/rustio.jar -s -o /dev/null -X POST $BASE/admin/posts/new \
  -d "_csrf=$CSRF&title=Hello&body=World&published=on&created_at=2026-04-24T12:00"

# 6. Search for it (should appear after ~200ms indexer flush)
sleep 1
hits=$(curl -s "$BASE/search?q=Hello" | grep -o '"total":[0-9]*' | cut -d: -f2)
test "$hits" -gt 0 || { echo "search returned 0 hits"; exit 1; }

echo "smoke test passed"
