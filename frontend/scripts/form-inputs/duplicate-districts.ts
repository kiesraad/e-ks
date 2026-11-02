// A district that already belongs to another candidate list stays selectable,
// but picking it puts that district on two lists. Mark the selected option
// and show the message that names the other list.
export default function duplicateDistricts() {
  const options = [
    ...document.querySelectorAll<HTMLElement>(
      "#district_list .checkbox[data-on-other-list]",
    ),
  ];
  if (options.length === 0) {
    return;
  }
  const fieldset = options[0].closest("fieldset");

  const update = () => {
    for (const option of options) {
      const input = option.querySelector<HTMLInputElement>(
        'input[type="checkbox"]',
      );
      const selected = input?.checked === true;
      option.classList.toggle("error", selected);
      fieldset
        ?.querySelector(
          `.district-duplicate-warning[data-district="${option.dataset.onOtherList}"]`,
        )
        ?.classList.toggle("hidden", !selected);
    }
  };

  // Changes of a district checkbox and of "select all" (which ticks the
  // boxes itself, without firing their events) both bubble up to the fieldset.
  fieldset?.addEventListener("change", update);
  update();
}
