interface StatTileProps {
  /** Preformatted display value, e.g. "9.7×" or "32,677" */
  value: string;
  label: string;
  delta?: { text: string; good: boolean };
}
declare function StatTile(props: StatTileProps): JSX.Element;
