interface BadgeProps {
  /** pass/fail/wip = bordered rect badges; supported/partial/unsupported = leading-dot format-support states */
  status: "pass" | "fail" | "wip" | "supported" | "partial" | "unsupported";
  /** Defaults to status uppercased */
  label?: string;
}
declare function Badge(props: BadgeProps): JSX.Element;
