interface TagChipProps {
  /** Hex offset prefix, e.g. "0x0110" — rendered dim */
  hex?: string;
  label: string;
  /** neutral = plain; group = orange (tag group names); value = green (value present); error = red */
  tone?: "neutral" | "group" | "value" | "error";
}
declare function TagChip(props: TagChipProps): JSX.Element;
