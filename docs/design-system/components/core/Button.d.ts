interface ButtonProps {
  /** primary = orange fill/black text; secondary = orange border+text; ghost = dim text */
  variant?: "primary" | "secondary" | "ghost";
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}
declare function Button(props: ButtonProps): JSX.Element;
