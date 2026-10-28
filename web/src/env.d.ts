// For tools that read .ts files with plain TypeScript (ESLint's type checker). vue-tsc types the real file.
declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent
  export default component
}
