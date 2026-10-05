// Passkey (WebAuthn) ceremonies for the CSB login page and the passkey
// management page. A form marked with `data-passkey-login-start` or
// `data-passkey-register-start` posts its fields as JSON to the start URL,
// hands the returned options to the browser's credential API, posts the
// resulting credential to the finish URL and then navigates to
// `data-passkey-redirect`. Without JavaScript the forms show a notice instead.

type Kind = "login" | "register";

function csrfHeaders(): Record<string, string> {
  // Only the management page has a session (and the meta tag); the login
  // page's requests are guarded by the fetch-metadata layer instead.
  const token = document
    .querySelector('meta[name="csrf-token"]')
    ?.getAttribute("content");
  return token ? { "X-CSRF-Token": token } : {};
}

function postJson(url: string, body: unknown): Promise<Response> {
  return fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...csrfHeaders() },
    body: JSON.stringify(body),
  });
}

function formPayload(form: HTMLFormElement): Record<string, string> {
  const payload: Record<string, string> = {};
  for (const [key, value] of new FormData(form).entries()) {
    if (typeof value === "string") {
      payload[key] = value;
    }
  }
  return payload;
}

function showError(form: HTMLFormElement, conflict: boolean) {
  const generic = form.querySelector<HTMLElement>("[data-passkey-error]");
  const taken = form.querySelector<HTMLElement>(
    "[data-passkey-error-conflict]",
  );
  generic?.classList.toggle("hidden", conflict && taken !== null);
  taken?.classList.toggle("hidden", !conflict);
}

function hideErrors(form: HTMLFormElement) {
  for (const element of form.querySelectorAll<HTMLElement>(
    "[data-passkey-error], [data-passkey-error-conflict]",
  )) {
    element.classList.add("hidden");
  }
}

function supported(): boolean {
  return (
    typeof PublicKeyCredential !== "undefined" &&
    "parseCreationOptionsFromJSON" in PublicKeyCredential &&
    "parseRequestOptionsFromJSON" in PublicKeyCredential
  );
}

async function obtainCredential(
  kind: Kind,
  options: { publicKey: unknown },
): Promise<PublicKeyCredential | null> {
  const credential =
    kind === "register"
      ? await navigator.credentials.create({
          publicKey: PublicKeyCredential.parseCreationOptionsFromJSON(
            options.publicKey as PublicKeyCredentialCreationOptionsJSON,
          ),
        })
      : await navigator.credentials.get({
          publicKey: PublicKeyCredential.parseRequestOptionsFromJSON(
            options.publicKey as PublicKeyCredentialRequestOptionsJSON,
          ),
        });
  return credential instanceof PublicKeyCredential ? credential : null;
}

async function runCeremony(
  form: HTMLFormElement,
  kind: Kind,
  startUrl: string,
  finishUrl: string,
) {
  hideErrors(form);
  if (!supported()) {
    showError(form, false);
    return;
  }

  const started = await postJson(startUrl, formPayload(form));
  if (!started.ok) {
    showError(form, started.status === 409);
    return;
  }

  const credential = await obtainCredential(kind, await started.json());
  if (!credential) {
    showError(form, false);
    return;
  }

  const finished = await postJson(finishUrl, credential.toJSON());
  if (finished.status !== 204) {
    showError(form, false);
    return;
  }

  window.location.assign(form.dataset.passkeyRedirect ?? window.location.href);
}

function attach(form: HTMLFormElement, kind: Kind) {
  const startUrl =
    kind === "login"
      ? form.dataset.passkeyLoginStart
      : form.dataset.passkeyRegisterStart;
  const finishUrl =
    kind === "login"
      ? form.dataset.passkeyLoginFinish
      : form.dataset.passkeyRegisterFinish;
  if (!startUrl || !finishUrl) {
    return;
  }

  const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]');
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (submit) {
      submit.disabled = true;
    }
    runCeremony(form, kind, startUrl, finishUrl)
      .catch((error) => {
        // The user cancelled, or the authenticator refused.
        console.error("Passkey ceremony failed", error);
        showError(form, false);
      })
      .finally(() => {
        if (submit) {
          submit.disabled = false;
        }
      });
  });
}

export default function setupPasskey() {
  for (const form of document.querySelectorAll<HTMLFormElement>(
    "form[data-passkey-login-start]",
  )) {
    attach(form, "login");
  }
  for (const form of document.querySelectorAll<HTMLFormElement>(
    "form[data-passkey-register-start]",
  )) {
    attach(form, "register");
  }
}
