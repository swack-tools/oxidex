function BenchmarkBar({ items, max }) {
  const top = max || Math.max(...items.map((i) => i.value));
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "12px", width: "100%" }}>
      <style>{"@media (prefers-reduced-motion: no-preference) { .ox-bb-fill { transition: width 400ms ease; } }"}</style>
      {items.map((item, i) => (
        <div key={i} style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          <span style={{ width: "140px", flex: "none", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", color: "var(--ox-text)" }}>{item.label}</span>
          <div style={{ flex: 1, height: "20px", background: "var(--ox-surface-2)", borderRadius: "var(--ox-radius-chip)" }}>
            <div
              className="ox-bb-fill"
              style={{
                width: `${(item.value / top) * 100}%`,
                height: "100%",
                background: item.highlight ? "var(--ox-accent)" : "rgba(139, 147, 163, 0.4)",
                borderRadius: "var(--ox-radius-chip)",
              }}
            ></div>
          </div>
          <span style={{ width: "80px", flex: "none", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", color: "var(--ox-text)", textAlign: "right" }}>
            {item.value}
            {item.unit}
          </span>
        </div>
      ))}
    </div>
  );
}

Object.assign(window, { BenchmarkBar });
