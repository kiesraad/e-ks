// A server-rendered field error describes the value it was submitted with, so
// drop it once the user has edited that value and left the field.
export default function staleErrors() {
  for (const field of document.querySelectorAll<HTMLElement>(".form-field")) {
    const errors = [...field.querySelectorAll<HTMLElement>("span.error")];
    if (errors.length === 0) {
      continue;
    }

    // Checkboxes in a field (autoformat initials, "no BSN") are not its value.
    const inputs = [
      ...field.querySelectorAll<
        HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
      >('input:not([type="checkbox"]), select, textarea'),
    ];
    if (inputs.length === 0) {
      continue;
    }

    const submitted = inputs.map((input) => input.value);
    const dropIfEdited = () => {
      const edited = inputs.some(
        (input, index) => input.value !== submitted[index],
      );
      if (!edited) {
        return;
      }

      for (const error of errors) {
        error.remove();
      }
      for (const input of inputs) {
        input.removeEventListener("blur", dropIfEdited);
      }
    };

    for (const input of inputs) {
      input.addEventListener("blur", dropIfEdited);
    }
  }
}
