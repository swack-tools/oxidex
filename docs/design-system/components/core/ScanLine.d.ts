interface ScanLineProps {
  /** 0–1. Omit for indeterminate divider mode. */
  progress?: number;
  label?: string;
}
declare function ScanLine(props: ScanLineProps): JSX.Element;
