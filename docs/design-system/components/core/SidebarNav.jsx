function SidebarNav({ sections }) {
  return (
    <div className="ox-sn" style={{ width: "220px", flex: "none" }}>
      <style>{"/* inline color needs the !important to lose to hover */ .ox-sn .ox-sn-dim:hover { color: var(--ox-text) !important; }"}</style>
      {sections.map((section, s) => (
        <div key={s} style={{ marginTop: s === 0 ? 0 : "24px" }}>
          <div style={{ fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-12)", letterSpacing: "var(--ox-track-label)", textTransform: "uppercase", color: "var(--ox-text)", padding: "0 12px", marginBottom: "8px" }}>
            {section.title}
          </div>
          {section.items.map((item, i) => (
            <div
              key={i}
              className={item.active ? "ox-sn-item" : "ox-sn-item ox-sn-dim"}
              style={{
                fontFamily: "var(--ox-font-mono)",
                fontSize: "var(--ox-fs-13)",
                padding: "6px 12px",
                cursor: "pointer",
                color: item.active ? "var(--ox-accent)" : "var(--ox-text-dim)",
                borderLeft: item.active ? "2px solid var(--ox-accent)" : "2px solid transparent",
                background: item.active ? "var(--ox-surface)" : "none",
              }}
            >
              {item.label}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

Object.assign(window, { SidebarNav });
