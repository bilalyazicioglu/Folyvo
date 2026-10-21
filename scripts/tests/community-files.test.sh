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
has "readme links the contribution guide" README.md "CONTRIBUTING.md"

exists .github/ISSUE_TEMPLATE/bug.yml
exists .github/ISSUE_TEMPLATE/feature.yml
exists .github/ISSUE_TEMPLATE/config.yml
exists .github/PULL_REQUEST_TEMPLATE.md
exists .github/CODEOWNERS
exists .github/dependabot.yml

has "blank issues are disabled" .github/ISSUE_TEMPLATE/config.yml "blank_issues_enabled: false"
has "issue config links to discussions" .github/ISSUE_TEMPLATE/config.yml "https://github.com/bilalyazicioglu/Folyvo/discussions"
has "issue config links to private security reporting" .github/ISSUE_TEMPLATE/config.yml "https://github.com/bilalyazicioglu/Folyvo/security/advisories/new"
has "pull request template has the consent box" .github/PULL_REQUEST_TEMPLATE.md "- [ ] I have the right to submit this and agree that it is licensed under GPL-3.0-or-later."
has "pull request template asks how it was tested" .github/PULL_REQUEST_TEMPLATE.md "How was it tested"
has "code owners cover everything" .github/CODEOWNERS "* @bilalyazicioglu"
has "dependabot updates github actions" .github/dependabot.yml "package-ecosystem: github-actions"
has "dependabot commit prefix satisfies the commit lint" .github/dependabot.yml "prefix: build(deps)"

exists docs/development/desktop.md
has "docs index links the desktop guide" docs/README.md "development/desktop.md"
has "desktop guide explains the unsigned first launch" docs/development/desktop.md "Open Anyway"
has "desktop guide documents the vault override" docs/development/desktop.md "FOLYVO_VAULT"
has "desktop guide has the manual demo checklist" docs/development/desktop.md "Manual demo"
has "contributing mentions the UI tests" CONTRIBUTING.md "npm --prefix apps/desktop/ui test"

yaml_files=(.github/ISSUE_TEMPLATE/bug.yml .github/ISSUE_TEMPLATE/feature.yml .github/ISSUE_TEMPLATE/config.yml .github/dependabot.yml)
for f in "${yaml_files[@]}"; do
  if [ -f "$root/$f" ] && ruby -ryaml -e 'YAML.safe_load(File.read(ARGV[0]))' "$root/$f" 2>/dev/null; then pass "$f parses as YAML"; else fail "$f parses as YAML"; fi
done

finish
