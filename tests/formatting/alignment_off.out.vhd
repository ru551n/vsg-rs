entity e is
  generic (
    g_long_name : natural := 1;
    g : natural := 2
  );
  port (
    clk : in bit;
    data_out : out bit
  );
end entity e;

architecture a of e is

begin

  u : entity work.f
    port map (
      clk => clk,
      data_out => data_out
    );

end architecture a;

entity c_user is
end entity c_user;

architecture a of c_user is

  component f is
    port (
      clk : in bit;
      data_out : out bit
    );
  end component f;

  procedure p (constant a_long : in integer; constant b : in integer; variable result_value : out integer) is
  begin

  end procedure p;

  signal x : bit; -- short
  signal long_name : bit; -- long

begin

end architecture a;
