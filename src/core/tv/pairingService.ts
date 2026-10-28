import { supabase } from "@/integrations/supabase/client";
import { secretStorage } from "./secretStorage";
import {
  payloadSchema,
  unbase64,
  openTransfer,
  type SealedTransfer,
  type TransferPayload,
} from "./pairingCrypto";
export { sealTransfer, openTransfer } from "./pairingCrypto";
export type { SealedTransfer, TransferPayload } from "./pairingCrypto";

export type PairingRequest = {
  id: string;
  pin: string;
  pollSecret: string;
  phrase: string;
  expiresAt: string;
};
export type PairingLookup = Pick<
  PairingRequest,
  "id" | "phrase" | "expiresAt"
> & { publicKey: JsonWebKey };
export const PAIRING_UNAVAILABLE_MESSAGE =
  "TV pairing is not available in this build.";
/** TV pairing needed the upstream TatakaiAPI backend, which this fork does not use. */
export async function pairingAPI<T>(
  _operation: string,
  _body: unknown,
  _authenticated = false,
  _signal?: AbortSignal
): Promise<T> {
  throw new Error(PAIRING_UNAVAILABLE_MESSAGE);
}
export async function createPairing(signal?: AbortSignal) {
  const keys = await crypto.subtle.generateKey(
    {
      name: "RSA-OAEP",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    false,
    ["encrypt", "decrypt"]
  );
  const publicKey = await crypto.subtle.exportKey("jwk", keys.publicKey);
  const request = await pairingAPI<PairingRequest>(
    "create",
    { publicKey },
    false,
    signal
  );
  return { request, privateKey: keys.privateKey };
}
export async function receivePairing(
  privateKey: CryptoKey,
  payload: { login: string; transfer: SealedTransfer }
) {
  const login = JSON.parse(
    new TextDecoder().decode(
      await crypto.subtle.decrypt(
        "RSA-OAEP",
        privateKey,
        unbase64(payload.login)
      )
    )
  );
  if (login.type !== "magiclink") throw new Error("Invalid TV login token");
  const transfer = await openTransfer(privateKey, payload.transfer);
  const { error } = await supabase.auth.verifyOtp({
    token_hash: login.tokenHash,
    type: "magiclink",
  });
  if (error) throw error;
  return transfer;
}
export async function importTransfer(value: TransferPayload) {
  const payload = payloadSchema.parse(value) as TransferPayload;
  const failures: string[] = [];
  for (const [provider, credential] of Object.entries(
    payload.credentials || {}
  )) {
    try {
      await secretStorage.set(`provider:${provider}`, credential);
      if (provider === "real-debrid" || provider === "torbox") {
        const { debridOrchestrator } = await import(
          "@/core/providers/debrid-orchestrator"
        );
        const name = provider === "real-debrid" ? "realdebrid" : "torbox";
        const accounts = debridOrchestrator
          .getAccounts()
          .filter((a) => a.provider !== name);
        debridOrchestrator.saveAccounts([
          ...accounts,
          {
            provider: name,
            apiKey: credential,
            isActive: accounts.length === 0,
          },
        ]);
      }
    } catch {
      failures.push(provider);
    }
  }
  for (const playlist of payload.playlists || []) {
    try {
      const { db } = await import("@/core/db/tatakai-db");
      // A retry does not duplicate a successfully imported playlist.
      const existing = (await db.iptvPlaylists.toArray()).find(
        (p) => p.name === playlist.name
      );
      await (
        await import("@/core/iptv/iptvStorage")
      ).savePlaylist(playlist.name, playlist.source, existing);
    } catch {
      failures.push(playlist.name);
    }
  }
  for (const extension of payload.extensions || []) {
    try {
      const marketplace = await import("@/core/extensions/marketplace-client");
      const trusted = (await marketplace.fetchMarketplaceExtensions()).find(
        (item) => item.id === extension.id && item.mainUrl === extension.url
      );
      if (!trusted)
        throw new Error("Extension is not in the approved marketplace");
      const result = await (
        await import("@/core/extensions/mobile/mobileExtensionInstaller")
      ).installMobileKaiExtension(
        await marketplace.downloadExtensionKai(trusted.mainUrl),
        trusted
      );
      if (!result.success) throw new Error("Extension validation failed");
    } catch {
      failures.push(extension.id);
    }
  }
  return failures;
}
