# Rule status

`vsg-rs --list_rules` prints the status of every VSG rule and `vsg-rs -rc RULE` its
configuration. This page summarizes them.

VSG 3.35.0 has 972 rules:

| Status | Rules | Meaning |
|---|---|---|
| implemented | 192 | reported by `vsg-rs`, with the fix class listed below |
| formatter | 779 | layout policy applied by `vsg-rs --fix` (whitespace, indentation, blank lines, alignment, keyword case, line structure); configured through the VSG rule options described in `formatting.md` |
| command line | 1 | `source_file_001`: a missing input file is an error (exit code 1) |

Formatter-owned rules are not reported one by one: unformatted lines are reported as `format`
violations, and `vsg-rs --fix --diff` shows the change.

## Fix classes

* **safe**: applied by `--fix`.
* **unsafe**: applied only with `--fix --unsafe_fixes`.
* **none**: reported only.

`--fix` applies the fixes VSG applies: a rule whose VSG default is `fixable: false` (for
example `port_023`) is fixed only with `--unsafe_fixes`, unless the configuration sets
`fixable: true` for it. The table shows the fix itself; `fixable: false` rules are marked in
VSG's configuration (`vsg-rs -rc RULE`).

Identifier case fixes are always safe, because VHDL identifiers (other than extended
identifiers, which are never changed) are case-insensitive. The consistency rules
(`signal_014`, `constant_013`, …) use the spelling that the declaration's own case rule asks
for, so a single `fix` run leaves nothing to fix. They compare names within the file:
declarations and uses in other files are not checked.

## Implemented rules

Defaults and severities follow VSG's defaults (all errors except `length_001` and
`comment_012`).

| Rule | Description | Default | Fix |
|---|---|---|---|
| `after_001` | Assignments in the clock branch of clock processes have an `after` delay | off | unsafe |
| `after_003` | Assignments in the reset branch of clock processes have no `after` delay | off | unsafe |
| `alias_declaration_502` | Alias designators are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `alias_declaration_503` | Uses of an alias repeat the declared spelling | on | safe |
| `alias_declaration_600` | Alias designators have a valid prefix | off | none |
| `alias_declaration_601` | Alias designators have a valid suffix | off | none |
| `architecture_010` | `end` of an architecture includes the `architecture` keyword | on | safe |
| `architecture_011` | The name after `end architecture` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `architecture_013` | Architecture names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `architecture_014` | The entity name of an architecture is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `architecture_024` | `end` of an architecture repeats the architecture name | on | safe |
| `architecture_025` | Architecture names are one of the configured names | off | none |
| `architecture_600` | Uses of generics in an architecture repeat the declared spelling | on | safe |
| `architecture_601` | Uses of ports in an architecture repeat the declared spelling | on | safe |
| `attribute_500` | Predefined attributes are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `attribute_declaration_501` | Attribute names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `attribute_declaration_502` | Attribute type marks are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `attribute_specification_501` | Attribute designators are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `block_002` | Block statements include the optional `is` | on | safe |
| `block_007` | `end block` repeats the block label | on | safe |
| `block_500` | Block labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `block_506` | The label after `end block` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `block_600` | Block labels have a valid suffix | off | none |
| `block_601` | Block labels have a valid prefix | off | none |
| `block_comment_001` | Block comment headers follow the configured pattern | off | none |
| `block_comment_002` | Block comment lines start with the configured text | off | none |
| `block_comment_003` | Block comment footers follow the configured pattern | off | none |
| `case_019` | Case statements have no label | on | safe (unsafe if the label is referenced) |
| `case_020` | `end case` has no label | on | safe |
| `comment_011` | Comments are not placed after code | off | safe |
| `comment_012` | Comments do not contain the configured keywords | off | none |
| `component_008` | Component names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `component_012` | The name after `end component` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `component_019` | Component port and generic clauses have no trailing comments | on | unsafe |
| `component_021` | Component declarations include the optional `is` | on | safe |
| `component_022` | `end component` repeats the component name | on | safe |
| `concurrent_005` | Concurrent signal assignments have no label | on | safe (unsafe if the label is referenced) |
| `constant_004` | Constant names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `constant_013` | Uses of a constant repeat the declared spelling | on | safe |
| `constant_015` | Constant names have a valid prefix | off | none |
| `constant_600` | Constant names have a valid suffix | off | none |
| `context_012` | Context names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `context_016` | The name after `end context` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `context_021` | `end` of a context includes the `context` keyword | on | safe |
| `context_022` | `end` of a context repeats the context name | on | safe |
| `context_ref_009` | Context references name one context each | on | safe |
| `context_ref_500` | Library names in context references are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `context_ref_501` | Context names in context references are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `entity_008` | Entity names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `entity_012` | The name after `end entity` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `entity_015` | `end` of an entity includes the `entity` keyword | on | safe |
| `entity_019` | `end` of an entity repeats the entity name | on | safe |
| `entity_600` | Uses of generics in an entity repeat the declared spelling | on | safe |
| `exponent_500` | Exponents of decimal literals are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `file_500` | File object names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `file_open_information_501` | File open kinds are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `function_010` | Calls of a function repeat the declared spelling | on | safe |
| `function_017` | Function designators are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `function_018` | `end` of a function body includes `function` | on | safe |
| `function_020` | `end` of a function body repeats its designator | on | safe |
| `function_506` | The designator after `end function` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `function_507` | Function parameter names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `function_508` | Uses of parameters in a function body repeat the declared spelling | on | safe |
| `function_600` | Function designators have a valid prefix | off | none |
| `function_601` | Function designators have a valid suffix | off | none |
| `generate_005` | Generate labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `generate_011` | `end generate` repeats the generate label | on | safe |
| `generate_012` | The label after `end generate` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `generate_017` | Generate labels have a valid prefix | off | none |
| `generate_600` | Generate labels have a valid suffix | off | none |
| `generate_601` | Generate parameters have a valid prefix | off | none |
| `generate_602` | Generate parameters have a valid suffix | off | none |
| `generic_007` | Generic names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `generic_020` | Generic names have a valid prefix | off | none |
| `generic_600` | Generic names have a valid suffix | off | none |
| `generic_map_002` | Generic names in generic maps are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `generic_map_008` | Generic maps use named association | on | none |
| `generic_map_600` | Generic names in generic maps have a valid suffix | off | none |
| `generic_map_601` | Generic names in generic maps have a valid prefix | off | none |
| `if_002` | If and elsif conditions are enclosed in parentheses | on | safe |
| `instantiation_008` | Instance labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `instantiation_009` | Component names in instantiations are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `instantiation_028` | Entity names in direct instantiations are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `instantiation_033` | Component instantiations include `component` (`action: remove` for the opposite) | on | safe |
| `instantiation_034` | Instantiations use the configured `method` (component or entity) | on | none |
| `instantiation_036` | Entity instantiations name the architecture (`action: remove` for the opposite) | on | unsafe (`remove`), none (`add`) |
| `instantiation_500` | Library names in direct instantiations are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `instantiation_600` | Instance labels have a valid suffix | off | none |
| `instantiation_601` | Instance labels have a valid prefix | off | none |
| `interface_incomplete_type_declaration_501` | Generic type names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `interface_incomplete_type_declaration_600` | Generic type names have a valid prefix | off | none |
| `interface_incomplete_type_declaration_601` | Generic type names have a valid suffix | off | none |
| `length_001` | Lines must not be longer than the configured length | on | by `--fix` (folding) |
| `library_012` | Restricted libraries are not used | off | none |
| `library_500` | Library names in library clauses are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `loop_statement_006` | Loop statements have a label | off | none |
| `loop_statement_007` | `end loop` repeats the loop label | off | safe |
| `loop_statement_503` | Loop labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `loop_statement_504` | The label after `end loop` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `loop_statement_600` | Loop labels have a valid prefix | off | none |
| `loop_statement_601` | Loop labels have a valid suffix | off | none |
| `loop_statement_602` | Loop parameters have a valid prefix | off | none |
| `loop_statement_603` | Loop parameters have a valid suffix | off | none |
| `package_007` | `end` of a package includes the `package` keyword | on | safe |
| `package_008` | The name after `end package` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_010` | Package names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_014` | `end` of a package repeats the package name | on | safe |
| `package_016` | Package names have a valid suffix | off | none |
| `package_017` | Package names have a valid prefix | off | none |
| `package_body_002` | `end` of a package body includes `package body` | on | safe |
| `package_body_003` | `end` of a package body repeats the package name | on | safe |
| `package_body_502` | Package body names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_body_507` | The name after `end package body` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_body_600` | Package body names have a valid suffix | off | none |
| `package_body_601` | Package body names have a valid prefix | off | none |
| `package_instantiation_501` | Instantiated package names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_instantiation_504` | Uninstantiated package names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `package_instantiation_600` | Instantiated package names have a valid suffix | off | none |
| `package_instantiation_601` | Instantiated package names have a valid prefix | off | none |
| `parameter_specification_500` | Loop and generate parameters are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `port_010` | Port names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `port_011` | Port names have a valid prefix | off | none |
| `port_012` | Ports have no default values | on | unsafe |
| `port_023` | Port declarations have a mode | on | safe |
| `port_025` | Port names have a valid suffix | off | none |
| `port_026` | Port declarations declare one port each | on | safe |
| `port_600` | Input port names have a valid prefix | off | none |
| `port_601` | Output port names have a valid prefix | off | none |
| `port_602` | Inout port names have a valid prefix | off | none |
| `port_603` | Buffer port names have a valid prefix | off | none |
| `port_604` | Linkage port names have a valid prefix | off | none |
| `port_605` | Input port names have a valid suffix | off | none |
| `port_606` | Output port names have a valid suffix | off | none |
| `port_607` | Inout port names have a valid suffix | off | none |
| `port_608` | Buffer port names have a valid suffix | off | none |
| `port_609` | Linkage port names have a valid suffix | off | none |
| `port_map_002` | Port names in port maps are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `port_map_008` | Port maps use named association | on | none |
| `port_map_010` | Port and generic maps have no trailing comments | on | unsafe |
| `procedure_012` | `end` of a procedure body includes `procedure` | on | safe |
| `procedure_014` | `end` of a procedure body repeats its designator | on | safe |
| `procedure_501` | Procedure designators are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `procedure_506` | The designator after `end procedure` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `procedure_507` | Calls of a procedure repeat the declared spelling | on | safe |
| `procedure_508` | Procedure parameter names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `procedure_509` | Uses of parameters in a procedure body repeat the declared spelling | on | safe |
| `procedure_call_001` | Procedure calls have no label | on | safe (unsafe if the label is referenced) |
| `procedure_call_002` | Concurrent procedure calls have no label | on | safe (unsafe if the label is referenced) |
| `procedure_call_500` | Procedure call labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `procedure_call_502` | Formal parameter names in procedure calls are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `process_012` | Process statements include the optional `is` | on | safe |
| `process_016` | Process statements have a label | on | none |
| `process_017` | Process labels are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `process_018` | `end process` repeats the process label | on | safe |
| `process_019` | The label after `end process` is in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `process_029` | Clock conditions use the configured `clock` style (`event` or `edge`) | off | unsafe |
| `process_036` | Process labels have a valid prefix | off | none |
| `process_600` | Process labels have a valid suffix | off | none |
| `record_type_definition_005` | `end record` repeats the record type name | on | safe |
| `report_statement_001` | Report statements have no label | on | safe (unsafe if the label is referenced) |
| `reserved_001` | Words reserved in any VHDL standard are not used as identifiers | on | none |
| `sequential_006` | Sequential signal assignments contain no comments | on | none |
| `signal_004` | Signal names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `signal_007` | Signal declarations have no default value | on | unsafe |
| `signal_008` | Signal names have a valid prefix | off | none |
| `signal_014` | Uses of a signal repeat the declared spelling | on | safe |
| `signal_015` | Signal declarations declare at most `consecutive` signals | on | safe |
| `signal_600` | Signal names have a valid suffix | off | none |
| `subprogram_instantiation_500` | Instantiated subprogram names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `subprogram_instantiation_503` | Uninstantiated subprogram names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `subtype_002` | Uses of a subtype repeat the declared spelling | on | safe |
| `subtype_004` | Subtype names have a valid prefix | off | none |
| `subtype_501` | Subtype names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `subtype_600` | Subtype names have a valid suffix | off | none |
| `type_004` | Type names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `type_014` | Uses of a type repeat the declared spelling | on | safe |
| `type_015` | Type names have a valid prefix | off | none |
| `type_500` | Enumeration literals are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `type_501` | Uses of enumeration literals repeat the declared spelling | on | safe |
| `type_600` | Type names have a valid suffix | off | none |
| `type_mark_500` | Type marks of types not declared in the file are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `use_clause_001` | Restricted packages are not used | off | none |
| `use_clause_500` | Library names in use clauses are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `use_clause_501` | Package names in use clauses are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `use_clause_502` | Item names in use clauses are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `variable_004` | Variable names are in the configured case | on | safe (report only for camelCase, PascalCase, regex) |
| `variable_007` | Variable declarations have no default value | on | unsafe |
| `variable_011` | Uses of a variable repeat the declared spelling | on | safe |
| `variable_012` | Variable names have a valid prefix | off | none |
| `variable_015` | Variable declarations declare at most `consecutive` variables | on | safe |
| `variable_600` | Variable names have a valid suffix | off | none |
| `variable_assignment_006` | Variable assignments contain no comments | on | none |

## Not supported

* The settings in VSG's `indent.tokens` block that do not change a construct's indentation —
  continuation-line offsets, for example. The rest of the block is replayed; see
  [formatting](formatting.md#indentation-indenttokens).
* VSG's `local_rules` (Python rule plugins) are not run by vsg-rs itself but by an installed VSG
  (see `compatibility.md`).
* Several `case::keyword` rules name the same keyword in different constructs (for example
  `end`); vsg-rs applies one case per keyword and warns when the configured cases conflict.
