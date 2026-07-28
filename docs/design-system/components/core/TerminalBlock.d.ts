interface TerminalLine {
  /** true = command line (gets $ prefix, command word in orange) */
  prompt?: boolean;
  text: string;
}
interface TerminalBlockProps {
  lines: TerminalLine[];
  /** Title-bar text, default "oxidex" */
  title?: string;
}
declare function TerminalBlock(props: TerminalBlockProps): JSX.Element;
