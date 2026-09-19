-- vsg-rs-test: width=80
library ieee;
  use ieee.std_logic_1164.all;
  use ieee.numeric_std.all;

entity fifo is
  generic (
    width      : positive                     := 8;
    depth      : positive                     := 16;
    init_value : std_logic_vector(7 downto 0) := (others => '0')
  );
  port (
    clk, rst : in    std_logic; -- clock and reset
    -- write side
    wr_en    : in    std_logic;
    wr_data  : in    std_logic_vector(width - 1 downto 0);
    rd_data  : out   std_logic_vector(width - 1 downto 0);
    count    : buffer natural range 0 to depth;
    bidir    : inout std_logic
  );
end fifo;
