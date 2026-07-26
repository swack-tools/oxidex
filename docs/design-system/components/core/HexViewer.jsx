function HexViewer({ bytes, baseOffset = 0, highlight }) {
  const rows = [];
  for (let i = 0; i < bytes.length; i += 16) rows.push(bytes.slice(i, i + 16));
  const inHl = (idx) => highlight && idx >= highlight.start && idx < highlight.end;
  const hlStyle = { background: "rgba(232, 130, 74, 0.2)", color: "var(--ox-accent)" };
  return (
    <div>
      <div
        style={{
          background: "var(--ox-surface)",
          border: "1px solid var(--ox-border)",
          borderRadius: "var(--ox-radius)",
          padding: "16px",
          overflowX: "auto",
          fontFamily: "var(--ox-font-mono)",
          fontSize: "var(--ox-fs-13)",
          lineHeight: 1.7,
        }}
      >
        {rows.map((row, r) => (
          <div key={r} style={{ display: "flex", whiteSpace: "nowrap" }}>
            <span style={{ width: "80px", flex: "none", color: "var(--ox-text-dim)" }}>
              {(baseOffset + r * 16).toString(16).padStart(8, "0")}
            </span>
            <span style={{ display: "inline-flex" }}>
              {row.map((b, c) => (
                <span
                  key={c}
                  style={{
                    width: "22px",
                    textAlign: "center",
                    marginRight: c === 7 ? "8px" : 0,
                    color: "var(--ox-text)",
                    ...(inHl(r * 16 + c) ? hlStyle : {}),
                  }}
                >
                  {b.toString(16).padStart(2, "0")}
                </span>
              ))}
            </span>
            <span style={{ marginLeft: "16px", display: "inline-flex" }}>
              {row.map((b, c) => (
                <span key={c} style={{ width: "9px", textAlign: "center", color: "var(--ox-text-dim)", ...(inHl(r * 16 + c) ? hlStyle : {}) }}>
                  {b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : "·"}
                </span>
              ))}
            </span>
          </div>
        ))}
      </div>
      {highlight && highlight.label ? (
        <div style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-12)", color: "var(--ox-accent)", marginTop: "6px" }}>
          {"└─ "}
          {highlight.label}
        </div>
      ) : null}
    </div>
  );
}

Object.assign(window, { HexViewer });
