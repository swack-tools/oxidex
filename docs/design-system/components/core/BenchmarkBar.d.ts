interface BenchmarkBarItem {
  label: string;
  value: number;
  unit: string;
  /** true = orange (us); false/absent = border-gray (them) */
  highlight?: boolean;
}
interface BenchmarkBarProps {
  items: BenchmarkBarItem[];
  /** Defaults to the max item value */
  max?: number;
}
declare function BenchmarkBar(props: BenchmarkBarProps): JSX.Element;
