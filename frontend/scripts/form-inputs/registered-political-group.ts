// Fill the general information from the committee registration picked and
// lock the fields that registration decides over; unlock them again when the
// pick is cleared. The server takes the values from the registration on
// submit regardless, so the lock is a courtesy, not a safeguard.
export default function registeredPoliticalGroup() {
  const select = document.querySelector<HTMLSelectElement>(
    'select[name="registered_political_group"]',
  );
  if (!select) {
    return;
  }
  const form = select.closest("form") ?? document;
  const appellation = form.querySelector<HTMLInputElement>(
    'input[name="appellation"]',
  );
  const results = form.querySelectorAll<HTMLInputElement>(
    'input[name="previous_election_results"]',
  );

  select.addEventListener("change", () => {
    const option = select.selectedOptions[0];
    const locked = select.value !== "";
    if (locked && option) {
      if (appellation) {
        appellation.value = option.dataset.appellation ?? "";
      }
      results.forEach((input) => {
        input.checked = input.value === option.dataset.previousElectionResults;
      });
    }
    if (appellation) {
      appellation.readOnly = locked;
    }
    results.forEach((input) => {
      input.disabled = locked;
    });
  });
}
