<script setup lang="ts">
// Accounts are created by the Actual importer; the list arrives with it. Until then, the empty
// state (docs/spec/m1-ledger-import.md#accounts).
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p class="text-sm text-muted-foreground">No accounts yet.</p>
    <div class="flex flex-wrap gap-2">
      <RouterLink to="/import" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Import from Actual
      </RouterLink>
    </div>
  </div>
</template>
