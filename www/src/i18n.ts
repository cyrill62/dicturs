import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import { en } from './locales/en'
import { fr } from './locales/fr'

export const LANGUAGES = { en: 'English', fr: 'Français' } as const
export type Language = keyof typeof LANGUAGES
const STORAGE_KEY = 'dicturs.lang'

// Pages are prerendered in English; the browser switches to the visitor's language after hydration.
i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, fr: { translation: fr } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  returnObjects: true,
})

export function detectLanguage(): Language {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved && saved in LANGUAGES) return saved as Language
  } catch {
    // Storage unavailable (private mode): fall back to the browser language.
  }
  const browser = navigator.language.slice(0, 2)
  return browser in LANGUAGES ? (browser as Language) : 'en'
}

export function setLanguage(lang: Language) {
  i18n.changeLanguage(lang)
  document.documentElement.lang = lang
  try {
    localStorage.setItem(STORAGE_KEY, lang)
  } catch {
    // Not persisted: fine.
  }
}

export default i18n
