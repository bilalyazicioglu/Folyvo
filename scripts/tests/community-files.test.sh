#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
root="$(cd "$here/../.." && pwd)"

has() { # name file pattern   (fixed string, case-sensitive)
  if [ -f "$root/$2" ] && grep -qF -- "$3" "$root/$2"; then pass "$1"; else fail "$1"; fi
}
lacks() { # name file pattern
  if [ -f "$root/$2" ] && ! grep -qF -- "$3" "$root/$2"; then pass "$1"; else fail "$1"; fi
}
exists() { if [ -f "$root/$1" ]; then pass "$1 exists"; else fail "$1 exists"; fi; }

exists LICENSE
if [ -f "$root/LICENSE" ] && [ "$(wc -l < "$root/LICENSE")" -ge 670 ]; then pass "LICENSE is the full text"; else fail "LICENSE is the full text"; fi
has "LICENSE is GPL version 3" LICENSE "GNU GENERAL PUBLIC LICENSE"
has "LICENSE names version 3" LICENSE "Version 3, 29 June 2007"

has "code of conduct is the Contributor Covenant" CODE_OF_CONDUCT.md "Contributor Covenant"
has "code of conduct names the reporting address" CODE_OF_CONDUCT.md "conduct@bilalyazicioglu.com"
lacks "code of conduct has no placeholder left" CODE_OF_CONDUCT.md "[INSERT CONTACT METHOD]"

has "contributing states the licence of contributions" CONTRIBUTING.md "By contributing you agree that your contribution is licensed under GPL-3.0-or-later."
has "contributing explains commit conventions" CONTRIBUTING.md "Conventional Commits"
has "contributing points to the security policy" CONTRIBUTING.md "SECURITY.md"
has "contributing points to the code of conduct" CONTRIBUTING.md "CODE_OF_CONDUCT.md"

has "security policy has the private reporting link" SECURITY.md "https://github.com/bilalyazicioglu/Folyvo/security/advisories/new"
has "security policy has the alias" SECURITY.md "security@bilalyazicioglu.com"

has "support points to discussions" SUPPORT.md "https://github.com/bilalyazicioglu/Folyvo/discussions"

has "readme states the licence" README.md "GPL-3.0-or-later"
has "readme is honest about the status" README.md "under development"
has "readme explains release verification" README.md "gh attestation verify"
has "readme links the contribution guide" README.md "CONTRIBUTING.md"

finish
