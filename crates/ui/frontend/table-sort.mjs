// table-sort.mjs — pure sort-order computation for client-side table
// sorting in Markdown windows (window.js). No DOM access here: given the
// raw data-sort-value strings for one column (in current row order) plus
// a direction and a type, returns the ORIGINAL indices in the new order.
// Missing values (null — window.js passes null when a <td> has no
// data-sort-value attribute) always sort last, in EITHER direction.

export function sortedOrder(keys, dir, type) {
  const idx = keys.map((_, i) => i);
  const present = idx.filter((i) => keys[i] !== null);
  const missing = idx.filter((i) => keys[i] === null);
  const cmp = type === "num"
    ? (a, b) => parseFloat(keys[a]) - parseFloat(keys[b])
    : (a, b) => keys[a].localeCompare(keys[b]);
  present.sort((a, b) => (dir === "asc" ? cmp(a, b) : -cmp(a, b)));
  return [...present, ...missing];
}
