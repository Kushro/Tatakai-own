// Dashboard strings. The dashboard has no i18n system: the host texts (pairing, D8 info block)
// come straight from the app locales (extensionServerHost.*, contract 13 §3.11) and the few
// dashboard-only strings live here, in English and Spanish.
import { extensionServerHost as hostEn } from '../../src/locales/en.json';
import { extensionServerHost as hostEs } from '../../src/locales/es.json';

export type Lang = 'en' | 'es';

export const lang: Lang = /^es\b/i.test(typeof navigator === 'undefined' ? '' : navigator.language || '') ? 'es' : 'en';

const strings = {
  en: {
    serverOrigin: 'Server address',
    copy: 'Copy',
    copied: 'Copied',
    copyCode: 'Copy code',
    codeCopied: 'Code copied',
    generatePairing: 'Generate pairing QR',
    regeneratePairing: 'Regenerate pairing QR',
    pairingNotice: 'Pairing code expires in five minutes',
    qrLabel: 'Pairing QR code',
    deepLinkLabel: 'Pairing link',
    bootstrapHintDesktop: 'In Tatakai on this PC, open the extension server settings and use "Copy setup secret".',
    advancedHint: 'Advanced: you can also add the server by hand with the address above and the viewer password, if one is set.',
  },
  es: {
    serverOrigin: 'Dirección del servidor',
    copy: 'Copiar',
    copied: 'Copiado',
    copyCode: 'Copiar código',
    codeCopied: 'Código copiado',
    generatePairing: 'Generar QR de emparejamiento',
    regeneratePairing: 'Regenerar QR de emparejamiento',
    pairingNotice: 'El código de emparejamiento caduca en cinco minutos',
    qrLabel: 'Código QR de emparejamiento',
    deepLinkLabel: 'Enlace de emparejamiento',
    bootstrapHintDesktop: 'En Tatakai, en esta PC, abre los ajustes del servidor de extensiones y usa "Copiar secreto de configuración".',
    advancedHint: 'Avanzado: también puedes añadir el servidor a mano con la dirección de arriba y la contraseña de viewer, si hay una.',
  },
} as const;

export const t = strings[lang];
export const host = lang === 'es' ? hostEs : hostEn;

/** Replaces `{{name}}` placeholders (i18next style, as in the app locales). */
export function fmt(template: string, values: Record<string, string | number>) {
  return template.replace(/\{\{\s*(\w+)\s*\}\}/g, (match, key: string) => (key in values ? String(values[key]) : match));
}

export function formatCountdown(totalSeconds: number) {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
