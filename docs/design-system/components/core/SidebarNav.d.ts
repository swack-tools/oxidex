interface SidebarNavSection {
  title: string;
  items: { label: string; active?: boolean }[];
}
interface SidebarNavProps {
  sections: SidebarNavSection[];
}
declare function SidebarNav(props: SidebarNavProps): JSX.Element;
