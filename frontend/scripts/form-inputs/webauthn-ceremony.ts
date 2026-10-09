// Runs the WebAuthn ceremony on the CSB login page: asks the browser for an
// assertion with the options the server embedded in the form, and posts the
// result back as JSON in the hidden `credential` field.

function fromBase64url(value: string): ArrayBuffer {
  const binary = atob(value.replaceAll("-", "+").replaceAll("_", "/"));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
}

function toBase64url(buffer: ArrayBuffer): string {
  let binary = "";
  for (const byte of new Uint8Array(buffer)) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary)
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");
}

// The server sends every binary field base64url-encoded; the browser API
// wants buffers.
function requestOptions(
  options: Record<string, unknown>,
): PublicKeyCredentialRequestOptions {
  const descriptors = options.allowCredentials as Array<{
    id: string;
    type: string;
    transports?: string[];
  }>;
  return {
    ...options,
    challenge: fromBase64url(options.challenge as string),
    allowCredentials: descriptors.map((descriptor) => ({
      ...descriptor,
      id: fromBase64url(descriptor.id),
    })),
  } as PublicKeyCredentialRequestOptions;
}

// The assertion as the server expects it.
function serialize(credential: PublicKeyCredential): Record<string, unknown> {
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: toBase64url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: toBase64url(response.authenticatorData),
      clientDataJSON: toBase64url(response.clientDataJSON),
      signature: toBase64url(response.signature),
    },
  };
}

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
  if (!window.PublicKeyCredential) {
    unsupportedAlert.classList.remove("hidden");
    retryButton.classList.add("hidden");
    return;
  }

  const options = JSON.parse(form.dataset.webauthnOptions ?? "{}");

  const run = async () => {
    errorAlert.classList.add("hidden");
    retryButton.classList.add("hidden");
    try {
      const credential = await navigator.credentials.get({
        publicKey: requestOptions(options.publicKey),
      });
      if (!(credential instanceof PublicKeyCredential)) {
        throw new Error("no credential");
      }
      credentialInput.value = JSON.stringify(serialize(credential));
      form.submit();
    } catch (error) {
      console.error(error);
      errorAlert.classList.remove("hidden");
      retryButton.classList.remove("hidden");
    }
  };

  retryButton.addEventListener("click", (event) => {
    event.preventDefault();
    run();
  });
  if (form.dataset.webauthnAutostart !== "false") {
    run();
  }
}
