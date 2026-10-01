const calloutKinds = {
  note: { signal: "var(--ox-blue)", label: "NOTE" },
  warn: { signal: "var(--ox-accent)", label: "WARN" },
  fail: { signal: "var(--ox-red)", label: "FAIL" },
};

function Callout({ kind = "note", children }) {
  const k = calloutKinds[kind] || calloutKinds.note;
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: "12px",
        padding: "12px 16px",
        background: "var(--ox-surface)",
        border: "1px solid var(--ox-border)",
        borderLeft: `2px solid ${k.signal}`,
        borderRadius: "var(--ox-radius)",
      }}
    >
      <span style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-12)", letterSpacing: "var(--ox-track-label)", color: k.signal, flex: "none" }}>{k.label}</span>
      <span style={{ fontFamily: "var(--ox-font-body)", fontSize: "var(--ox-fs-15)", lineHeight: 1.6, color: "var(--ox-text)" }}>{children}</span>
    </div>
  );
}

Object.assign(window, { Callout });
