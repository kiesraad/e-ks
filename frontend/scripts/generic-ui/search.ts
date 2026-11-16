// Filter table rows based on the search input value.
export default function setupTextSearch() {
  const search = document.getElementById("search") as HTMLInputElement | null;
  const tableIds = ["add-candidate-table", "persons-table"];
  const table = tableIds
    .map((id) => document.getElementById(id))
    .find(Boolean) as HTMLTableElement | null;

  if (!search || !table) {
    return;
  }

  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
    }
  });

  search.addEventListener("input", (e) => {
    const searchValue = (e.target as HTMLInputElement).value.toLowerCase();
    const rows = table.querySelectorAll("tbody tr");

    rows.forEach((element: Element) => {
      const row = element as HTMLTableRowElement;
      const rowText = row.textContent?.toLowerCase() || "";

      row.hidden = !rowText.includes(searchValue);
    });
  });
}
