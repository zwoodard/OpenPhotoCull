// Distinct colors for duplicate groups (high saturation, good contrast on dark bg)
export const GROUP_COLORS = [
  "#818cf8", // indigo
  "#f472b6", // pink
  "#34d399", // emerald
  "#fbbf24", // amber
  "#60a5fa", // blue
  "#a78bfa", // violet
  "#fb923c", // orange
  "#2dd4bf", // teal
  "#f87171", // red
  "#4ade80", // green
  "#e879f9", // fuchsia
  "#38bdf8", // sky
];

export function dupGroupLabel(groupId: string): string {
  const num = groupId.replace(/\D/g, "");
  return num || groupId.slice(0, 4);
}

export function dupGroupColor(groupId: string): string {
  const num = parseInt(groupId.replace(/\D/g, ""), 10) || 0;
  return GROUP_COLORS[(num - 1) % GROUP_COLORS.length];
}
