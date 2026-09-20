# Native rules in detail

The twenty-two rules vsg-rs implements itself, as opposed to the resolved-semantic rules it gets from the
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
**Off by default** — enable it deliberately:

```yaml
rule:
  lint_700:
    disable: false
```

It is experimental: clock inference is the one place vsg-rs guesses at design intent rather than
deriving it, so it is not part of a default run.

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

<a id="lint_701"></a>
## lint_701 — Reset domain crossing

**Detects** a register reset by one signal used in logic reset by another. Off by default.

**Why it matters** the two resets can be released at different moments. The source stops being
held while the destination is still in reset, or the other way round, and the destination
captures a value from a register that is mid-release. An asynchronous release close to the clock
edge is a recovery or removal violation, which is metastability by another name.

It is the same shape as a clock domain crossing and the same shape of mistake, with the domains
drawn by resets rather than clocks. `lint_700` finds one, `lint_701` the other.

**Evidence** the reset each clocked process clears its registers with, and the signals each
process reads. **Heuristic**: the rule decides which signal is a reset, which the source does not
state. **Context** one architecture with at least two resets. **Severity** error. **Fix** none.
**Off by default**, like `lint_700`:

```yaml
rule:
  lint_701:
    disable: false
```

```vhdl
  p_b : process (clk, rst_b) is
  begin
    if rst_b = '1' then
      out_b <= '0';
    elsif rising_edge(clk) then
      out_b <= in_a and d;      -- in_a is reset by rst_a
    end if;
  end process p_b;
```

```text
lint_701 | Error | 28 | Signal 'in_a' is reset by 'rst_a' and used in logic reset by 'rst_b'
```

Accepted, because it is how the crossing is made safe:

```vhdl
      in_b  <= in_a;            -- captured into this domain first
      out_b <= in_b and d;
```

**Limitations** only an asynchronous reset counts, written as the branch the clock edge is the
`elsif` of. That is not only caution: the hazard is a reset asserting or releasing away from the
clock edge, and a synchronous reset is ordinary clocked logic that crosses nothing. Reading the
first test inside `if rising_edge(clk) then` also cannot tell a reset from a clock enable, which
is exactly the mistake an earlier version made. A register cleared by two different resets is
left alone rather than guessed at.

---

<a id="lint_702"></a>
## lint_702 — Register the reset branch does not clear

**Detects** a register a clocked process assigns, that its reset branch does not. Off by default.

**Why it matters** the reset branch says what the design starts from. A register missing from it
starts from whatever it happens to hold. Often that is an oversight; sometimes it is deliberate,
because a wide data path costs more to reset than the reset is worth. The rule states the fact
and leaves the judgement, which is why it is advisory.

**Evidence** the assignment targets in the reset branch, against those in the clocked branch.

**Context** none. **Severity** error. **Fix** none. **Off by default.**

```vhdl
    if rst = '1' then
      a <= '0';                 -- b is not cleared
    elsif rising_edge(clk) then
      a <= d;
      b <= d;
    end if;
```

```text
lint_702 | Error | 13 | Signal 'b' is assigned by this process but not by its 'rst' branch
```

**Limitations** the reset has to be asynchronous, written as the branch the clock edge is the
`elsif` of, because that is the only place a reset can be told from a clock enable. A process
with a synchronous reset is not judged.

---

<a id="lint_703"></a>
## lint_703 — Clock used on both edges

**Detects** one clock driving registers on the rising edge in one place and the falling edge in
another. Off by default.

**Why it matters** it halves the time available between the two sets of registers. Usually it is
a mistake about which signal was meant; sometimes it is the design, as in double data rate logic
or an SPI interface that samples on one edge and shifts on the other.

**Evidence** the edge each clocked process is triggered on.

**Context** none. **Severity** error. **Fix** none. **Off by default.**

```text
lint_703 | Error | 195 | Clock 'm2s.sck' is used on both edges in this architecture
```

**Limitations** an edge inside a `wait` is a process suspending, not a register, and is not
counted: a testbench that waits on one edge and drives on the other is ordinary. The whole name
is compared, so `m2s.sck` and `m2s.cs_n` are two clocks rather than one.

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

<a id="lint_741"></a>
## lint_741: Vector comparison width mismatch

**Detects** a vector compared with one of a different width.

**Why it matters** array equality is defined as equal lengths with matching elements, so
comparing a four-bit vector with a three-bit literal is not an error: it is simply always false.
The design elaborates, the simulation runs, and the branch is never taken. GHDL says nothing at
all; NVC warns.

**Evidence** the declared range of the object and the length of the literal or the other object,
when both are certain.

**Context** none. **Severity** error. **Fix** none.

```vhdl
    a : in bit_vector(3 downto 0);
  ...
  q <= '1' when a = "000" else '0';
```

```text
lint_741 | Error | 5 | 'a' is 4 bits and is compared with '"000"', which is 3, so the
                       comparison is always false
```

**Limitations** the same restraint as `lint_740`. Both sides must be a whole object or a vector
literal, so a slice, an index, a concatenation or a function call is left alone. A string whose
characters are not `std_logic`'s is text rather than a vector and is never measured, so a
`report` message is safe from it.

---

<a id="lint_712"></a>
## lint_712 — Unreachable `when others`

**Detects** a `when others` alternative on a case whose other alternatives already name every
value of the selector's type.

**Why it matters** the branch cannot be taken today, so it reads as dead code. What it does is
change what happens tomorrow: add a value to the enumeration and the case is exhaustive again for
the wrong reason — the new value falls into `others`, silently doing whatever that branch does,
commonly `null`. Without the `others`, the same edit is a compile error naming the choice that is
missing, which is the answer the author wanted.

**Evidence** the enumeration's declared values, and the choices of every alternative.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  type state_t is (idle, run, done);
  signal state : state_t;
  ...
  case state is
    when idle   => null;
    when run    => null;
    when done   => null;
    when others => null;
  end case;
```

```text
lint_712 | Error | 14 | 'when others' can never be taken: the other alternatives already
                        name all 3 values of 'state'
```

**Limitations** reports only where the branch is provably dead. The selector must be a plain name
of an object declared with an enumeration this file declares, every value of that enumeration must
be a plain identifier, and every choice must be one of those values. A selector that is an
expression, a choice that is a range or a constant, a character-literal value, or a name declared
twice with different types all disqualify the case rather than being guessed at.

---

<a id="lint_713"></a>
## lint_713 — `when others` instead of naming every value

**Detects** a `when others` alternative that stands in for values of an enumeration the case does
not name. Off by default.

**Why it matters** the same future edit as `lint_712`, arrived at from the other side: while
`others` is there, adding a value to the enumeration compiles, and the new value quietly takes the
`others` branch. A project that wants every value spelled out enables this and gets a compile
error instead. This is a house's choice rather than a defect, which is why it is off unless asked
for.

**Evidence** the enumeration's declared values, and the choices of every alternative.

**Context** none. **Severity** error. **Fix** none.

```yaml
rule:
  lint_713:
    disable: false
```

```vhdl
  type state_t is (idle, run, done);
  signal state : state_t;
  ...
  case state is
    when idle   => null;
    when run    => null;
    when others => null;
```

```text
lint_713 | Error | 13 | 'when others' covers 1 of the values of 'state' (done)
```

**Limitations** the same evidence as `lint_712`, so the same cases are out of reach. The two rules
divide the `others` alternatives between them and never report the same one twice: `lint_712` takes
those no value can reach, `lint_713` those that do stand in for values.

---

<a id="lint_750"></a>
## lint_750 — Component does not match its entity

**Detects** a component declaration whose ports differ from those of the entity of the same name:
a port one has and the other does not, a port with a different mode, or the same ports in a
different order.

**Why it matters** a component declaration is a hand-written copy of an entity's interface, and
copies drift. An instance binds to the *component*, so the design goes on elaborating and means
something other than it reads like — until the binding fails instead, usually much later and
somewhere else. Differing order is the worst of the three: a positional instantiation then
connects the wrong signals, and nothing about it looks wrong.

**Evidence** the port names and modes of both declarations, compared directly. Only entities the
run can see are compared, so a component standing for a vendor primitive is left alone.

**Context** the entity must be among the files being checked. **Severity** error. **Fix** none.

```vhdl
entity dut is
  port (clk : in bit; d : in bit; q : out bit);
end entity dut;
...
  component dut is                 -- q is missing, and d has become an output
    port (clk : in bit; d : out bit; spare : in bit);
  end component dut;
```

```text
lint_750 | Error | 9 | Component 'dut' does not match entity 'dut': it does not declare q;
                       and it declares, which the entity does not have, spare; and it gives
                       a different mode to d
```

**Limitations** compares names and modes, not types or widths — the analyser does not resolve
type marks across files. Entities are matched by bare name, so two entities of the same name in
different libraries are treated as one; this is the same assumption `lint_730` makes. A component
whose entity is not in the file set is not reported at all, rather than guessed at.

---

<a id="lint_751"></a>
## lint_751 — Configuration names an architecture that is not declared

**Detects** a configuration whose `for` clause, or whose `use entity` binding, names an
architecture that no file declares.

**Why it matters** a configuration is the one place a design names an architecture in writing,
and nothing checks the name until the design is elaborated. A simulator rejects it — but only
once someone runs one, which may be a build and a wait away. This is the difference between a
failed lint job and a failed build.

**Evidence** the architectures declared for each entity, gathered over every file of the run,
against the names the configuration uses.

**Context** the entity must be among the files being checked. **Severity** error. **Fix** none.

```vhdl
configuration cfg of tb is
  for sim
    for i_dff : dff
      use entity work.dff(no_such_arch);
    end for;
  end for;
end configuration cfg;
```

```text
lint_751 | Error | 17 | Configuration binds to architecture 'no_such_arch' of 'dff', which
                        is not declared
```

It also checks the instances a configuration names: `for i_dff : dff` against what the
architecture really instantiates, so a label that no longer exists, or one whose instance is of
a different component, is reported.

**Limitations** only the configurations written directly in the architecture's own block. One
nested inside another configures a block or a generate, whose labels are its own. The unit is
compared only where the instantiation names it outright. An entity the run cannot see is left
alone entirely: from inside one run, "not here" and "not anywhere" look the same — and the rule
says nothing at all unless the run covers the whole project, because absence is evidence only
then.

---

<a id="lint_760"></a>
## lint_760 — Recursive subprogram

**Detects** a subprogram whose body calls itself. Off by default.

**Why it matters** recursion is legal VHDL and simulates perfectly. No synthesis tool accepts
it: the hardware for a call is inlined at the call site, and a call that reaches itself has no
bottom to inline from. A recursive function in a package that RTL also uses is a synthesis
failure waiting for whoever instantiates it, and the vendor tool reports it much later and much
less clearly.

It is off by default because recursion is perfectly reasonable in code that is only ever
simulated — OSVVM's alert hierarchy walks itself, and is right to. Enable it for code that has
to synthesise:

```yaml
rule:
  lint_760:
    disable: false
```

**Evidence** the resolved call graph: every call in a subprogram's body, resolved to the
subprogram it actually names. **Context** needs the library map, like every rule that resolves
names; see [project setup](project-setup.md). **Severity** error. **Fix** none.

```vhdl
  function fact (n : integer) return integer is
  begin
    if n <= 1 then
      return 1;
    end if;
    return n * fact(n - 1);
  end function fact;
```

```text
lint_760 | Error | 11 | Subprogram 'fact' calls itself
```

**This one cannot be written against the syntax.** A function whose body names itself is nearly
always calling a *different* subprogram of the same name: scanning VUnit for that shape finds
1871 candidates, almost none of them recursive, because VHDL overloads heavily — `to_slv`
calling another `to_slv` is the normal case. Only the resolved symbol table tells the two apart.

**Limitations** only *direct* recursion. VHDL requires a forward declaration for two subprograms
to call each other, and a call to a name that has one resolves to the declaration rather than to
the body, so a mutual cycle is not visible here. Under-reporting is the right way to be wrong
about this.

---

<a id="lint_770"></a>
## lint_770 — Process that can never suspend

**Detects** a process with no sensitivity list and no `wait` statement.

**Why it matters** a process repeats for ever. What stops it monopolising the simulation is that
it suspends: at a `wait`, or at the end of its body when it has a sensitivity list, which is the
same thing written differently. With neither, it reaches the end, starts again, and never yields
— time never advances and the simulation makes no progress. This is not a design that behaves
oddly; it is a design that cannot run.

Both simulators say so at analysis. GHDL: *"infinite loop for this process without a wait
statement"*. NVC: *"potential infinite loop in process with no sensitivity list and no wait
statements"*.

**Evidence** the process's own syntax: its sensitivity list, its `wait` statements, and the
procedure calls in its body.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  p : process
  begin
    x <= '1';
  end process p;
```

```text
lint_770 | Error | 8 | Process 'p' can never suspend: it has no sensitivity list and no
                       wait statement
```

**Limitations** NVC's *potential* is the reason for them. A process whose body calls a procedure
may suspend inside it — a procedure, unlike a function, is allowed to contain a `wait` — so a
process that calls one is left alone rather than resolved. A `wait` that looks unreachable still
counts, because deciding otherwise is a second proof this rule does not need and would risk
reporting a process that does suspend. Every form of sensitivity list counts, `process (all)`
included.

---

<a id="lint_771"></a>
## lint_771: Function that can fall through

**Detects** a function whose body can reach its end without returning a value.

**Why it matters** reaching the end of a function without a `return` is an error, and it is the
one this layer exists for: the compilers are silent, and GHDL reports `missing return in
function` only at run time, only if the run reaches the call, and only with an argument that
takes the silent path. The answer was in the source the whole time.

**Evidence** the statements of the body and which of them transfer control.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  function polynomial (width : natural) return bit_vector is
  begin
    case width is
      when 8 => return x"D5";
      when others => report "unsupported" severity error;
    end case;
  end function;
```

```text
lint_771 | Error | 1 | 'polynomial' can reach the end of its body without returning a
                       value, which is an error when it does
```

That example is open-logic's, near enough: the same function appears twice in that project, once
with a closing `return` and once without.

**Limitations** everything unclear counts as an ending, so the rule accuses nothing it cannot
show. A loop is assumed to return, since reasoning about whether one runs is a second proof this
rule does not need. `assert false` is an ending whatever severity it carries, because an author
who writes it has said the path cannot happen and a rule in the definite class does not argue
with that. A `case` needs no `when others`: the LRM requires its choices to cover the subtype, so
one that analyses at all is exhaustive, and an incomplete one is the front end's finding rather
than this one's.

---

<a id="lint_772"></a>
## lint_772: Statement nothing can reach

**Detects** a statement after one that always transfers control away.

**Why it matters** unreachable code is either a mistake or a leftover, and neither compiler
mentions it.

**Evidence** the position of a `return`, `exit` or `next` that carries no condition, and a
statement after it in the same list.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  function g (x : integer) return integer is
  begin
    return x;
    return x + 1;
  end function;
```

```text
lint_772 | Error | 4 | Nothing reaches this statement: the `return` above it always
                       transfers control
```

**Limitations** an assertion is not a transfer of control. It reports and execution carries on,
so defensive code after one is reached and is not reported, even at `severity failure`. `exit
when` and `next when` are conditional and transfer nothing this rule can rely on. A
`-- synthesis translate_off` region between the two statements is respected: code that is
unreachable in simulation and reachable in synthesis is how a design asks which of the two is
reading it, and `in_simulation` in hdl-modules is exactly that.

---

<a id="lint_780"></a>
## lint_780 — Index outside the array's range

**Detects** an index written as a literal that falls outside the literal range its array was
declared with.

**Why it matters** the index is subject to the array's index range, so evaluating it raises an
error. There is no reading under which the program carries on. Both simulators say so at
analysis — GHDL *"static expression violates bounds"*, NVC *"array X index 8 outside of NATURAL
range 7 downto 0"* — and the VHDL front end vsg-rs uses reports neither.

**Evidence** the declared range and the index, both written as integer literals.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  signal x : bit_vector(7 downto 0);
  ...
  y <= x(8);
```

```text
lint_780 | Error | 10 | Index 8 is outside the range 0 to 7 of 'x'
```

**Limitations** the exact case only. A range that mentions a generic or a constant is not
evaluated, and neither is an index that is anything but a literal — no value is propagated to
reach an answer. A slice is not an index. A name declared twice with different ranges is left
alone, because which declaration an index belongs to is a question about scope. `f(8)` is read
as an index only when `f` is an object this file declares with a range, so a function call of
the same shape is not reported.

---

<a id="lint_781"></a>
## lint_781 — Division by zero

**Detects** `/`, `mod` or `rem` whose right operand is written as zero.

**Why it matters** evaluating it raises an error: the LRM leaves the result undefined for a zero
right operand and requires the error. Unlike the index case, **neither GHDL nor NVC says
anything about this at analysis**, so nothing warns before the run reaches the statement.

**Evidence** the operator and a literal zero beside it.

**Context** none. **Severity** error. **Fix** none.

```vhdl
    n := n / 0;
```

```text
lint_781 | Error | 9 | Division by zero
```

**Limitations** a literal zero only. A named constant that happens to be zero needs its value
propagated to the division, and propagating values is how a rule stops being able to say what it
knows. `/=` is one token and is not a division.

---

<a id="lint_782"></a>
## lint_782 — Value outside the target's range

**Detects** an assignment of a literal that the target's declared range does not contain.

**Why it matters** the value has to belong to the subtype, so making the assignment raises an
error. Both simulators say so at analysis — GHDL *"expression constraints don't match target
ones"*, NVC *"value 20 outside of SMALL range 0 to 15 for variable V"* — and the VHDL front end
vsg-rs uses reports neither.

**Evidence** the declared range and the assigned value, both written as integer literals. The
range may be written on the object or on a subtype it names.

**Context** none. **Severity** error. **Fix** none.

```vhdl
  subtype small is integer range 0 to 15;
  ...
    variable v : small;
  ...
    v := 20;
```

```text
lint_782 | Error | 16 | 20 is outside the range 0 to 15 of 'v'
```

**Limitations** literals only, on both sides. A value that is a name or an expression is not
evaluated, and a range that depends on a generic is not either. A subtype of a subtype is not
followed.

---

## Next

* [Static analysis](lint.md) — the principle behind these rules
* [Rule reference](rule-reference.md) — every rule including the front end's
* [Waivers](waivers.md) — accepting a finding you have decided about
