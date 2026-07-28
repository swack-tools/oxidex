interface HexViewerProps {
  bytes: number[];
  /** Displayed offset of byte 0 (default 0) */
  baseOffset?: number;
  /** Half-open byte-index range to mark orange, with optional caption */
  highlight?: { start: number; end: number; label?: string };
}
declare function HexViewer(props: HexViewerProps): JSX.Element;
