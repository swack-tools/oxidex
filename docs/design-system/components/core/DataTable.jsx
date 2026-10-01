const dataTableKindColor = {
  offset: "var(--ox-text-dim)",
  key: "var(--ox-text-dim)",
  value: "var(--ox-green)",
  text: "var(--ox-text)",
};

function DataTable({ columns, rows }) {
  return (
    <div className="ox-dt" style={{ width: "100%" }}>
      <style>{".ox-dt tbody tr:hover { background: var(--ox-surface-2); }"}</style>
      <table style={{ width: "100%", borderCollapse: "collapse", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)" }}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th
                key={c.key}
                style={{
                  textAlign: "left",
                  padding: "8px 12px",
                  background: "var(--ox-surface-2)",
                  borderBottom: "1px solid var(--ox-border)",
                  fontFamily: "var(--ox-font-mono)",
                  fontSize: "var(--ox-fs-12)",
                  fontWeight: 400,
                  letterSpacing: "var(--ox-track-label)",
                  textTransform: "uppercase",
                  color: "var(--ox-text)",
                }}
              >
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i}>
              {columns.map((c) => (
                <td
                  key={c.key}
                  style={{
                    padding: "6px 12px",
                    borderBottom: "1px solid var(--ox-border)",
                    color: dataTableKindColor[c.kind || "text"],
                  }}
                >
                  {row[c.key]}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

Object.assign(window, { DataTable });
