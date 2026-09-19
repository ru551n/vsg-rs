//! Static analysis: what follows from the resolved program, as opposed to how it is laid out.
//!
//! The style layer ([`crate::rules`]) decides how source should look, one file at a time. This
//! layer asks whether the design says what its author meant: a signal nothing drives, two
//! statements driving one signal, a state a machine cannot leave. Some rules need only the syntax
//! tree; the rest need names resolved across files, which [`lint`] gets from the VHDL front end.
//!
//! Each rule states the evidence it works from, and reports nothing when that evidence is
//! missing. See `docs/lint.md`.

pub mod clockdomain;
pub mod combinational;
pub mod design;
pub mod elaborate;
pub mod fsm;
pub mod lint;
pub mod testbench;
pub mod width;
