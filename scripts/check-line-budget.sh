#!/bin/sh
set -eu

limit=1000
failed=0

{ git ls-files '*.rs'; git ls-files --others --exclude-standard '*.rs'; } |
  grep -v '^target/' |
  grep -v '^examples/app/src/models/' |
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    lines=$(wc -l < "$file")
    lines=${lines##*[!0-9]}

    if [ "$lines" -gt "$limit" ]; then
      printf '%s %s\n' "$lines" "$file"
      failed=1
    fi
  done > /tmp/vespertide-line-budget-offenders.txt

if [ -s /tmp/vespertide-line-budget-offenders.txt ]; then
  printf 'Rust files exceeding %s lines:\n' "$limit"
  cat /tmp/vespertide-line-budget-offenders.txt
  exit 1
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi

printf 'All tracked Rust files are within the %s-line budget.\n' "$limit"
