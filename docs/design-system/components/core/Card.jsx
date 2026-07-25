function Card({ title, hero, children }) {
  return (
    <div
      style={{
        position: "relative",
        background: "var(--ox-surface)",
        border: "1px solid var(--ox-border)",
        borderRadius: "var(--ox-radius)",
        padding: title ? "28px 24px 24px" : "24px",
        ...(hero ? { boxShadow: "var(--ox-glow)", borderColor: "rgba(232, 130, 74, 0.4)" } : {}),
      }}
    >
      {title ? (
        <span
          style={{
            position: "absolute",
            top: "-9px",
            left: "16px",
            fontFamily: "var(--ox-font-mono)",
            fontSize: "var(--ox-fs-13)",
            lineHeight: "18px",
            letterSpacing: "var(--ox-track-label)",
            textTransform: "uppercase",
            color: "var(--ox-text-dim)",
            background: "var(--ox-bg)",
            padding: "0 6px",
            whiteSpace: "nowrap",
          }}
        >
          {title}
        </span>
      ) : null}
      {children}
    </div>
  );
}

Object.assign(window, { Card });
