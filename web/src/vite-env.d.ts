/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_BASE?: string;
  /** Set to "1" by the public deploy to show the fake-mint notice at the top of every page. */
  readonly VITE_PUBLIC_DEMO?: string;
}
