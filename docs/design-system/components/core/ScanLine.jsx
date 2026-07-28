function ScanLine({ progress, label }) {
  const determinate = typeof progress === "number";
  return (
    <div style={{ width: "100%" }}>
      <style>{
        "@keyframes ox-scan { from { transform: translateX(-48px); } to { transform: translateX(calc(100vw)); } }" +
        "@media (prefers-reduced-motion: no-preference) { .ox-scan-shimmer { animation: ox-scan 2.4s linear infinite; } }"
      }</style>
      {label || determinate ? (
        <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "6px", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)" }}>
          <span style={{ color: "var(--ox-text-dim)" }}>{label || ""}</span>
          {determinate ? <span style={{ color: "var(--ox-green)" }}>{Math.round(progress * 100)}%</span> : null}
        </div>
      ) : null}
      <div style={{ position: "relative", height: "2px", background: "var(--ox-surface-2)", overflow: "hidden" }}>
        {determinate ? (
          <div style={{ position: "absolute", inset: 0, width: `${progress * 100}%`, background: "var(--ox-accent)" }}></div>
        ) : null}
        <div
          className="ox-scan-shimmer"
          style={{ position: "absolute", top: 0, bottom: 0, width: "48px", background: "linear-gradient(90deg, transparent, rgba(232, 130, 74, 0.6), transparent)" }}
        ></div>
      </div>
    </div>
  );
}

Object.assign(window, { ScanLine });
