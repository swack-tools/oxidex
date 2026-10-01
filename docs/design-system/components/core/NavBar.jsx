function NavBar({ items, logoText = "oxidex" }) {
  return (
    <div style={{ height: "56px", display: "flex", alignItems: "center", gap: "24px", padding: "0 24px", background: "var(--ox-surface)", borderBottom: "1px solid var(--ox-border)" }}>
      <span style={{ display: "inline-flex", alignItems: "center", gap: "8px", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-15)", marginRight: "16px" }}>
        <span style={{ width: "8px", height: "8px", background: "var(--ox-accent)", flex: "none" }}></span>
        <span style={{ color: "var(--ox-text)" }}>{logoText.slice(0, 3)}</span>
        <span style={{ color: "var(--ox-accent)", marginLeft: "-8px" }}>{logoText.slice(3)}</span>
      </span>
      {items.map((item, i) => (
        <span
          key={i}
          style={{
            fontFamily: "var(--ox-font-mono)",
            fontSize: "var(--ox-fs-13)",
            letterSpacing: "var(--ox-track-label)",
            textTransform: "uppercase",
            color: item.active ? "var(--ox-text)" : "var(--ox-text-dim)",
            height: "56px",
            display: "inline-flex",
            alignItems: "center",
            borderBottom: item.active ? "2px solid var(--ox-accent)" : "2px solid transparent",
            marginBottom: "-1px",
            cursor: "pointer",
          }}
        >
          {item.label}
        </span>
      ))}
    </div>
  );
}

Object.assign(window, { NavBar });
