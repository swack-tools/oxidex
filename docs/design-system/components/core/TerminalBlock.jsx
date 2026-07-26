function TerminalBlock({ lines, title = "oxidex" }) {
  const [copied, setCopied] = React.useState(false);
  const copyText = lines.filter((l) => l.prompt).map((l) => l.text).join("\n");
  const onCopy = () => {
    if (navigator.clipboard) navigator.clipboard.writeText(copyText);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  const renderPromptLine = (text) => {
    const sp = text.indexOf(" ");
    const cmd = sp === -1 ? text : text.slice(0, sp);
    const rest = sp === -1 ? "" : text.slice(sp);
    return (
      <span>
        <span style={{ color: "var(--ox-green)" }}>$ </span>
        <span style={{ color: "var(--ox-accent)" }}>{cmd}</span>
        <span style={{ color: "var(--ox-text)" }}>{rest}</span>
      </span>
    );
  };
  return (
    <div style={{ position: "relative", background: "#0a0c0e", border: "1px solid var(--ox-border)", borderRadius: "var(--ox-radius)", overflow: "hidden" }}>
      <div style={{ height: "28px", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--ox-surface)", borderBottom: "1px solid var(--ox-border)", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", color: "var(--ox-text-dim)" }}>
        {title}
      </div>
      <button
        onClick={onCopy}
        style={{ position: "absolute", top: "4px", right: "8px", background: "none", border: "none", cursor: "pointer", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-12)", letterSpacing: "var(--ox-track-label)", color: "var(--ox-accent)", padding: "4px" }}
      >
        {copied ? "COPIED" : "COPY"}
      </button>
      <div style={{ padding: "16px", fontFamily: "var(--ox-font-mono)", fontSize: "var(--ox-fs-13)", lineHeight: 1.7 }}>
        {lines.map((l, i) => (
          <div key={i} style={{ whiteSpace: "pre-wrap" }}>
            {l.prompt ? renderPromptLine(l.text) : <span style={{ color: "var(--ox-text-dim)" }}>{l.text}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}

Object.assign(window, { TerminalBlock });
