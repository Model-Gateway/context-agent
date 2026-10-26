type ClassValue = string | false | null | undefined;

/** Joins truthy class names; enough for this site without a merge library. */
export function cn(...values: ClassValue[]): string {
  return values.filter(Boolean).join(" ");
}
