const badgeSignals = {
  pass: { color: "var(--ox-green)", bg: "rgba(127, 216, 143, 0.12)" },
  fail: { color: "var(--ox-red)", bg: "rgba(224, 108, 117, 0.12)" },
  wip: { color: "var(--ox-text-dim)", bg: "rgba(139, 147, 163, 0.12)" },
};
const badgeDots = {
  supported: { background: "var(--ox-green)", opacity: 1 },
  partial: { background: "var(--ox-green)", opacity: 0.45 },
  unsupported: { background: "var(--ox-border)", opacity: 1 },
};

function Badge({ status, label }) {
  const text = (label || status).toUpperCase();
  if (badgeDots[status]) {
    return (
      <span style={{ display: "inline-flex", alignItems: "center", gap: "8px", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", color: "var(--ox-text)" }}>
        <span style={{ width: "8px", height: "8px", borderRadius: "50%", ...badgeDots[status] }}></span>
        {text}
      </span>
    );
  }
  const s = badgeSignals[status] || badgeSignals.wip;
  return (
    <span
      style={{
        display: "inline-block",
        padding: "2px 8px",
        borderRadius: "var(--ox-radius-chip)",
        border: `1px solid ${s.color}`,
        background: s.bg,
        fontFamily: "var(--ox-font-mono)",
        fontSize: "var(--ox-fs-12)",
        letterSpacing: "var(--ox-track-label)",
        color: status === "wip" ? "var(--ox-text)" : s.color,
      }}
    >
      {text}
    </span>
  );
}

Object.assign(window, { Badge });
