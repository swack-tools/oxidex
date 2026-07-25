interface CardProps {
  /** Renders a box-drawing-style header label interrupting the top border */
  title?: string;
  /** Adds the one-per-page orange glow */
  hero?: boolean;
  children: React.ReactNode;
}
declare function Card(props: CardProps): JSX.Element;
