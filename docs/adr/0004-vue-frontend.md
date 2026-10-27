# Vue 3 + TypeScript frontend on owned shadcn-vue components

The frontend is Vue 3 + TypeScript + Vite, built to static files that the axum server serves, using shadcn-vue (Reka UI) + Tailwind with the components copied into the repo and owned, and Apache ECharts for charts. Vue is the owner's familiar frontend stack, which matters because the owner QA's the generated code. Owning the components (rather than depending on a styled kit) keeps one set of design tokens in charge of every screen and chart, so the later styling pass changes tokens, not components. Layouts are mobile-first; the app is a responsive web app plus PWA, not a native app.

## Considered Options

- **React + a component kit**: larger ecosystem, but not the owner's stack.
- **A styled kit (Vuetify, PrimeVue)**: faster start, harder to make look like one designed system later.

## Consequences

- One server, one origin: no CORS, cookies just work.
- Frontend types come from the backend's OpenAPI document ([ADR-0007](0007-generated-api-contract.md)).
