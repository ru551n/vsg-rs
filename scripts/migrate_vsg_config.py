#!/usr/bin/env python3
"""Rewrite a VSG configuration written for 3.2x so that VSG 3.35 accepts it.

VSG 3.35 refuses to load a configuration that names a rule it has since renamed or merged, and
vsg-rs ignores those settings, so the successor rule runs at its default instead. Either way the
project is no longer linted the way it believes it is.

    python3 scripts/migrate_vsg_config.py old.yml > new.yml

The mapping is what VSG 3.35 itself prints when it rejects the configuration. A rule whose
successor is already configured under its own name is dropped rather than duplicated.
"""

import re
import sys

# deprecated rule -> successor, or None when the successor is configured elsewhere already
# (several old rules were merged into one).
SUCCESSOR = {
    "case_013": "null_statement_300",
    "case_generate_alternative_501": "choice_500",
    "function_014": "subprogram_kind_501",
    "procedure_009": "subprogram_kind_500",
    "procedure_505": None,  # also subprogram_kind_500
    "instantiation_001": "instantiation_300",  # also splits into generic_map_300
    "generic_017": "type_mark_500",
    "ieee_500": None,  # also type_mark_500
    "port_018": None,  # also type_mark_500
}

RULE = re.compile(r"^(\s+)([a-z_0-9]+):\s*$")


def migrate(text):
    out, dropping, seen = [], False, set()
    for line in text.splitlines(keepends=True):
        match = RULE.match(line)
        if match:
            indent, name = match.groups()
            if name in SUCCESSOR:
                successor = SUCCESSOR[name]
                if successor is None or successor in seen:
                    dropping = True
                    continue
                seen.add(successor)
                dropping = False
                out.append(f"{indent}{successor}:\n")
                continue
            seen.add(name)
            dropping = False
        if not dropping:
            out.append(line)
    return "".join(out)


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    with open(sys.argv[1], encoding="utf-8") as config:
        sys.stdout.write(migrate(config.read()))


if __name__ == "__main__":
    main()
