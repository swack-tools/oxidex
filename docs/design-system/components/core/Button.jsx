const buttonStyles = {
  base: {
    fontFamily: "var(--ox-font-mono)",
    fontSize: "var(--ox-fs-13)",
    letterSpacing: "var(--ox-track-label)",
    textTransform: "uppercase",
    padding: "8px 16px",
    borderRadius: "var(--ox-radius)",
    border: "1px solid transparent",
    cursor: "pointer",
    background: "none",
  },
  primary: {
    background: "var(--ox-accent)",
    color: "#0d0f12",
    borderColor: "var(--ox-accent)",
  },
  secondary: {
    color: "var(--ox-accent)",
    borderColor: "var(--ox-accent)",
  },
  ghost: {
    color: "var(--ox-text-dim)",
    borderColor: "transparent",
  },
};

function Button({ variant = "primary", children, onClick, disabled }) {
  const [pressed, setPressed] = React.useState(false);
  const style = {
    ...buttonStyles.base,
    ...buttonStyles[variant],
    ...(pressed && variant === "primary"
      ? { background: "var(--ox-accent-deep)", borderColor: "var(--ox-accent-deep)" }
      : {}),
    ...(disabled ? { opacity: 0.4, cursor: "not-allowed" } : {}),
  };
  return (
    <button
      style={style}
      onClick={disabled ? undefined : onClick}
      onMouseDown={() => setPressed(true)}
      onMouseUp={() => setPressed(false)}
      onMouseLeave={() => setPressed(false)}
    >
      {children}
    </button>
  );
}

Object.assign(window, { Button });
