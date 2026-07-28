const tagChipTones = {
  neutral: { label: "var(--ox-text)", border: "var(--ox-border)" },
  group: { label: "var(--ox-accent)", border: "rgba(232, 130, 74, 0.35)" },
  value: { label: "var(--ox-green)", border: "rgba(127, 216, 143, 0.35)" },
  error: { label: "var(--ox-red)", border: "rgba(224, 108, 117, 0.35)" },
};

function TagChip({ hex, label, tone = "neutral" }) {
  const t = tagChipTones[tone] || tagChipTones.neutral;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "baseline",
        gap: "6px",
        padding: "2px 8px",
        border: `1px solid ${t.border}`,
        borderRadius: "var(--ox-radius-chip)",
        fontFamily: "var(--ox-font-mono)",
        fontSize: "var(--ox-fs-13)",
        whiteSpace: "nowrap",
        background: "var(--ox-surface)",
      }}
    >
      {hex ? <span style={{ color: "var(--ox-text-dim)" }}>{hex}</span> : null}
      <span style={{ color: t.label }}>{label}</span>
    </span>
  );
}

Object.assign(window, { TagChip });
