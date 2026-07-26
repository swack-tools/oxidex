interface DataTableColumn {
  key: string;
  label: string;
  /** offset/key render dim, value renders green, text renders bright (default text) */
  kind?: "offset" | "key" | "value" | "text";
}
interface DataTableProps {
  columns: DataTableColumn[];
  rows: Record<string, React.ReactNode>[];
}
declare function DataTable(props: DataTableProps): JSX.Element;
