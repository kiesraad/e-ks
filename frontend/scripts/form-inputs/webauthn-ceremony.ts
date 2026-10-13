// Runs the WebAuthn ceremony on the CSB login page: asks the browser for an
// assertion with the options the server embedded in the form, and posts the
// result back as JSON in the hidden `credential` field. The browser does the
// base64url encoding both ways (Baseline 2025 JSON helpers).

export default function setupWebauthnCeremony() {
  const form = document.querySelector<HTMLFormElement>(
    "form[data-webauthn-options]",
  );
  if (!form) {
    return;
  }
  const credentialInput = form.querySelector<HTMLInputElement>(
    "input[name=credential]",
  );
  const errorAlert = form.querySelector<HTMLElement>("[data-webauthn-error]");
  const unsupportedAlert = form.querySelector<HTMLElement>(
    "[data-webauthn-unsupported]",
  );
  const retryButton = form.querySelector<HTMLButtonElement>(
    "[data-webauthn-retry]",
  );
  if (!credentialInput || !errorAlert || !unsupportedAlert || !retryButton) {
    return;
  }
  if (
    !window.PublicKeyCredential ||
    typeof PublicKeyCredential.parseRequestOptionsFromJSON !== "function"
  ) {
    unsupportedAlert.classList.remove("hidden");
    retryButton.classList.add("hidden");
    return;
  }

  const options = JSON.parse(form.dataset.webauthnOptions ?? "{}");

  // Never rejects: every failure shows the error and the retry button.
  const run = async () => {
    errorAlert.classList.add("hidden");
    retryButton.classList.add("hidden");
    try {
      const credential = await navigator.credentials.get({
        publicKey: PublicKeyCredential.parseRequestOptionsFromJSON(
          options.publicKey,
        ),
      });
      if (!(credential instanceof PublicKeyCredential)) {
        throw new TypeError("no credential");
      }
      // `JSON.stringify` calls `toJSON()`.
      credentialInput.value = JSON.stringify(credential);
      form.submit();
    } catch (error) {
      console.error(error);
      errorAlert.classList.remove("hidden");
      retryButton.classList.remove("hidden");
    }
  };

  retryButton.addEventListener("click", (event) => {
    event.preventDefault();
    void run();
  });
  if (form.dataset.webauthnAutostart !== "false") {
    void run();
  }
}
