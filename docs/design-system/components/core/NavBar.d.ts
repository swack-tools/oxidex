interface NavBarProps {
  items: { label: string; active?: boolean }[];
  /** Default "oxidex" — first 3 chars bright, rest orange */
  logoText?: string;
}
declare function NavBar(props: NavBarProps): JSX.Element;
