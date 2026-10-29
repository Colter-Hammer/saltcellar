<script setup lang="ts">
// Empty until the import lands; until then it offers the first steps *(agent)*.
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p class="text-sm text-muted-foreground">No transactions yet.</p>
    <div class="flex flex-wrap gap-2">
      <RouterLink to="/import" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Import from Actual
      </RouterLink>
      <RouterLink to="/categories" class="rounded-md border px-3 py-2">
        Set up categories
      </RouterLink>
    </div>
  </div>
</template>
