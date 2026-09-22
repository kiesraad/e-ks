// A server-rendered field error or warning describes the value it was submitted
// with, so drop it once the user has edited that value and left the field.
export default function staleErrors() {
  for (const field of document.querySelectorAll<HTMLElement>(".form-field")) {
    const messages = [
      ...field.querySelectorAll<HTMLElement>("span.error, span.warning"),
    ];
    const marked =
      field.classList.contains("error") || field.classList.contains("warning");
    if (messages.length === 0 && !marked) {
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

      // Hide rather than remove: scripts that own a message (the unknown
      // locality warning) keep toggling `hidden` on their own element.
      for (const message of messages) {
        message.classList.add("hidden");
      }
      field.classList.remove("error", "warning");

      for (const input of inputs) {
        input.removeEventListener("blur", dropIfEdited);
      }
    };

    for (const input of inputs) {
      input.addEventListener("blur", dropIfEdited);
    }
  }
}
