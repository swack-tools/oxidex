function StatTile({ value, label, delta }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
      <span style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-56)", fontWeight: 400, letterSpacing: "var(--ox-track-heading)", color: "var(--ox-text)", lineHeight: 1 }}>
        {value}
      </span>
      <span style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", letterSpacing: "var(--ox-track-label)", textTransform: "uppercase", color: "var(--ox-text-dim)" }}>
        {label}
      </span>
      {delta ? (
        <span style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", color: delta.good ? "var(--ox-green)" : "var(--ox-red)" }}>
          {delta.good ? "▲ " : "▼ "}
          {delta.text}
        </span>
      ) : null}
    </div>
  );
}

Object.assign(window, { StatTile });
