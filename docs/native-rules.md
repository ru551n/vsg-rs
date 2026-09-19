# Native rules in detail

The ten rules vsg-rs implements itself, as opposed to the resolved-semantic rules it gets from the
VHDL front end. Each entry says what evidence the analyser used, because that is what decides how
far to trust a finding.

Every example below is run against vsg-rs as part of preparing this page: the "reported" form
produces exactly the diagnostic shown, and the "accepted" form produces none.

The [rule reference](rule-reference.md) lists these alongside the front-end rules.

---

<a id="lint_600"></a>
## lint_600 — Latch inference

**Detects** a combinational process that leaves a signal unassigned on some path, which infers a
latch rather than the intended logic.

**Why it matters** a latch in synchronous logic is almost always a mistake: it makes timing
depend on the level of a control signal, and it is rarely what the author meant to write.

**Evidence** the assignments on each branch of the process, compared against the signals the
process assigns anywhere. Heuristic: the analyser must first decide the process is combinational,
which it infers from the absence of a clock edge and of `wait`.

**Context** none — runs without a library map. **Severity** error. **Fix** none.

```vhdl
  p_mux : process (sel, d) is
  begin
    if sel = '1' then
      q <= d;          -- q keeps its value when sel = '0'
    end if;
  end process p_mux;
```

```text
lint_600 | Error | 16 | Signal 'q' is not assigned on every path of this combinational
                        process, which infers a latch
```

Accepted — every path assigns `q`:

```vhdl
  p_mux : process (sel, d) is
  begin
    if sel = '1' then
      q <= d;
    else
      q <= '0';
    end if;
  end process p_mux;
```

An unconditional assignment before the branches works equally well, and is the usual way to write
combinational logic.

**Limitations** a process that suspends on `wait`, or that tests a clock edge, is never
considered. A signal assigned only inside a `for` loop is not reported, because the analyser does
not evaluate loop bounds.

---

<a id="lint_601"></a>
## lint_601 — Multiple drivers

**Detects** one signal assigned by more than one concurrent statement or process.

**Why it matters** two drivers on an unresolved type will not elaborate; on a resolved type such
as `std_logic` they will, and the result is decided by resolution rather than by the design.

**Evidence** the assignment targets of every concurrent statement, compared as designator paths,
so `q(3)` and `q(7 downto 4)` are understood to touch the same object while `q(3)` and `q(4)` are
not.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  q <= a;
  q <= b;
```

```text
lint_601 | Error | 13 | Signal 'q' is assigned by 2 concurrent statements (lines 13, 14)
```

**Limitations** deliberate tri-state buses are reported too: `std_logic` makes them legal, but a
second driver is far more often a mistake. Waive those. Branches of mutually exclusive generate
statements are not reported.

---

<a id="lint_602"></a>
<a id="lint_603"></a>
## lint_602, lint_603 — Register naming

**Detects** a signal assigned by a clocked process whose name lacks a configured suffix
(`lint_602`) or prefix (`lint_603`).

**Why it matters** it does not, to the hardware. This is a naming convention, not analysis — it
is listed here because it is implemented alongside the others, not because a finding says
anything about whether the design works.

**Evidence** the assignment targets of processes that test a clock edge, matched against the
configured affixes.

**Context** none. **Severity** error. **Fix** none. **Off unless configured.**

```yaml
rule:
  lint_602:
    disable: false
    suffixes: ['_r', '_q']
```

```text
lint_602 | Error | 18 | Registered signal 'counter' does not have the suffix '_r' or '_q'
```

Affixes may contain `*` and `?`, so `_p*` accepts `_p1` and `_p2`.

---

<a id="lint_700"></a>
## lint_700 — Unsynchronised clock domain crossing

**Detects** a register clocked by one clock, used in logic clocked by another, without passing
through a synchroniser.

**Why it matters** the receiving flop can sample the signal while it is changing, and go
metastable. It is one of the few bugs that passes simulation and fails in hardware.

**Evidence** the clock each process is edge-triggered on, and the signals each process assigns
and reads. **Heuristic**: the analyser infers which signal is a clock from `rising_edge`,
`falling_edge` and `'event`, and recognises a synchroniser by its shape.

**Context** one architecture with at least two clocks. **Severity** error. **Fix** none.

```vhdl
  p_b : process (clk_b) is
  begin
    if rising_edge(clk_b) then
      out_b <= flag_a and d;   -- flag_a is clocked by clk_a
    end if;
  end process p_b;
```

```text
lint_700 | Error | 27 | Signal 'flag_a' is registered on 'clk_a' and used in logic on
                        'clk_b': an unsynchronised clock domain crossing
```

Accepted — the crossing is captured before it is used:

```vhdl
      sync_b <= flag_a;
      out_b  <= sync_b and d;
```

Entities named in `vsg_rs: synchronizers` are also accepted, for projects with a CDC library:

```yaml
vsg_rs:
  synchronizers: ['cdc_bit_sync', 'xpm_cdc_*']
```

**Limitations** deliberately under-reports. A single-stage capture is accepted although two
stages are the usual requirement, because proving the second stage needs more context than one
architecture gives. A signal registered on two clocks is skipped entirely.

---

<a id="lint_710"></a>
## lint_710 — State never entered

**Detects** a state of an enumerated type that no transition ever assigns.

**Why it matters** it is dead logic, or a transition that was meant to exist and does not.

**Evidence** the values of the enumerated type, against every value assigned to signals of that
type.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  type state_t is (idle, running, aborted);   -- nothing ever assigns aborted
```

```text
lint_710 | Error | 11 | State 'aborted' of 'state_t' is never entered: nothing assigns it
```

**Limitations** the machine has to be readable with certainty: every assignment to the state must
be a plain value of the type. A state computed by a function is not analysed, and the type is
then left alone entirely.

---

<a id="lint_711"></a>
## lint_711 — State with no exit

**Detects** a state whose own `case` alternative never assigns a different state.

**Why it matters** the machine cannot leave it. Unless it is a deliberate terminal state, it is a
lock-up.

**Evidence** the `case` alternative for each state, and what it assigns to the state signal.

**Context** none. **Severity** error. **Fix** none.

```vhdl
        when stuck =>
          state <= stuck;      -- and nothing else
```

```text
lint_711 | Error | 31 | State 'stuck' of 'state_t' has no exit: its alternative never
                        assigns another state
```

**Limitations** a `case` that never assigns the state at all is a multiplexer selecting by state,
not transition logic, and is not checked. A deliberate terminal state — a halt state reached only
on a fatal error — is reported; waive it.

---

<a id="lint_720"></a>
## lint_720 — Combinational loop

**Detects** a signal that depends on itself through combinational logic, with no register in the
cycle.

**Why it matters** the hardware oscillates, or settles somewhere nobody chose.

**Evidence** the dependency graph of the architecture: every signal a combinational source reads,
pointing at every signal it drives. A clocked process contributes no edges, because the register
is what breaks the loop.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  grant   <= request and a;
  request <= grant or a;
```

```text
lint_720 | Error | 15 | Combinational loop: grant -> request -> grant depends on itself
                        with no register in the way
```

**Limitations** four things are deliberately left out so that a reported cycle is a real one:
assignments to part of an object (`q(0) <= q(1)` is two elements), anything through an instance
(whose entity may register the path), attributes (`q'length` is a property, not a value), and
assignments with an `after` delay.

---

<a id="lint_730"></a>
## lint_730 — Read but never driven

**Detects** a signal that something reads while nothing drives it: no assignment, and no instance
output.

**Why it matters** it holds its initial value forever. Usually a wire someone forgot to connect.

**Evidence** every assignment in the architecture, plus the port modes of every entity the run
can see, so a port map is read as "these signals are driven, those are read".

**Context** the entities involved must be among the files analysed. **Severity** error.
**Fix** none.

```vhdl
  signal enable : bit;          -- nothing ever assigns it
begin
  y <= a and enable;
```

```text
lint_730 | Error | 10 | Signal 'enable' is read but nothing drives it
```

**Limitations** nothing is assumed. An instance of an entity the run cannot see marks everything
it touches as driven; a procedure call marks every name it mentions as driven, because vsg-rs
does not resolve subprogram signatures; and a signal with an initial value is treated as tied on
purpose.

---

<a id="lint_740"></a>
## lint_740 — Vector width mismatch

**Detects** a vector assigned to one of a different width.

**Why it matters** both sides are `std_logic_vector`, so a type checker has nothing to say. The
lengths are compared only when the design elaborates, which means a simulator finds it and a
reader does not.

**Evidence** the declared ranges of both objects, when both are literal.

**Context** none. **Severity** error. **Fix** none.

```vhdl
    wide   : in  bit_vector(15 downto 0);
    narrow : out bit_vector(7 downto 0)
  ...
  narrow <= wide;
```

```text
lint_740 | Error | 12 | 'wide' is 16 bits wide and is assigned to 'narrow', which is 8
```

**Limitations** measures only what is certain: the statement must be `a <= b;` and nothing else,
both sides whole objects, both ranges literal. A slice, a concatenation, a conversion or a range
mentioning a generic is left alone, so most parameterised RTL is out of its reach by design.

---

## Next

* [Static analysis](lint.md) — the principle behind these rules
* [Rule reference](rule-reference.md) — every rule including the front end's
* [Waivers](waivers.md) — accepting a finding you have decided about
