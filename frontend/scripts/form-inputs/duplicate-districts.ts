// A district that already belongs to another candidate list stays selectable,
// but picking it puts that district on two lists. Mark the selected option so
// it and the warning above the list turn orange.
export default function duplicateDistricts() {
  const options = [
    ...document.querySelectorAll<HTMLElement>(
      "#district_list .checkbox[data-on-other-list]",
    ),
  ];
  if (options.length === 0) {
    return;
  }

  const update = () => {
    for (const option of options) {
      const input = option.querySelector<HTMLInputElement>(
        'input[type="checkbox"]',
      );
      option.classList.toggle("warning", input?.checked === true);
    }
  };

  // Changes of a district checkbox and of "select all" (which ticks the
  // boxes itself, without firing their events) both bubble up to the fieldset.
  options[0].closest("fieldset")?.addEventListener("change", update);
  update();
}
