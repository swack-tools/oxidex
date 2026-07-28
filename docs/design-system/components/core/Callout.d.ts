interface CalloutProps {
  /** note = blue; warn = orange; fail = red */
  kind: "note" | "warn" | "fail";
  children: React.ReactNode;
}
declare function Callout(props: CalloutProps): JSX.Element;
