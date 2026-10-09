#!/usr/bin/env bash
# Every `X.md` with a translation `X.<lang>.md` must keep the same shape:
# headings, table rows, code fences and links. Prints mismatches; exit 1 if any.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

count() { grep -c -E "$1" "$2" || true; }

fail=0
while IFS= read -r tr; do
  src="${tr%.*.md}.md"
  if [[ ! -f "$src" ]]; then
    echo "orphan translation (no $src): $tr"; fail=1; continue
  fi
  for kind in '^#{1,6} :headings' '^\|:table rows' '^```:code fences' '\]\(:links'; do
    re="${kind%%:*}"; name="${kind#*:}"
    a=$(count "$re" "$src"); b=$(count "$re" "$tr")
    if [[ "$a" != "$b" ]]; then
      echo "$tr: $name $b, $src has $a"; fail=1
    fi
  done
done < <(git ls-files --cached --others --exclude-standard '*.md' | grep -E '\.[a-z]{2}(-[A-Z]{2})?\.md$')

[[ $fail == 0 ]] && echo "docs in sync"
exit $fail
