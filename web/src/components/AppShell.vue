<script setup lang="ts">
// Phone: header + bottom tab bar. From `lg`: sidebar (docs/spec/foundations.md#frontend).
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import ServerBanner from '@/components/ServerBanner.vue'
import { sidebarItems, tabs } from '@/lib/nav'
import { cn } from '@/lib/utils'

const route = useRoute()
const title = computed(() => route.meta.title)
</script>

<template>
  <div class="min-h-dvh lg:flex">
    <nav
      aria-label="Sidebar"
      class="sticky top-0 hidden h-dvh w-60 shrink-0 flex-col gap-1 border-r bg-muted/40 p-3 lg:flex"
    >
      <div class="px-2 py-3 font-semibold">Saltcellar</div>
      <RouterLink
        v-for="item in sidebarItems"
        :key="item.to"
        v-slot="{ href, navigate, isActive }"
        :to="item.to"
        custom
      >
        <a
          :href="href"
          :aria-current="isActive ? 'page' : undefined"
          :class="
            cn(
              'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent',
              isActive && 'bg-accent font-medium',
            )
          "
          @click="navigate"
        >
          <component :is="item.icon" class="size-4" aria-hidden="true" />
          {{ item.label }}
        </a>
      </RouterLink>
    </nav>

    <div class="flex min-w-0 flex-1 flex-col">
      <header
        class="sticky top-0 z-30 flex h-12 items-center border-b bg-background px-4 lg:static lg:h-auto lg:border-none lg:px-6 lg:pt-6"
      >
        <h1 class="font-semibold lg:text-xl">{{ title }}</h1>
      </header>

      <ServerBanner />

      <main class="flex-1 pb-20 lg:pb-10">
        <slot />
      </main>

      <nav
        aria-label="Tabs"
        class="fixed inset-x-0 bottom-0 z-40 grid h-16 grid-cols-3 border-t bg-background pb-[env(safe-area-inset-bottom)] lg:hidden"
      >
        <RouterLink
          v-for="item in tabs"
          :key="item.to"
          v-slot="{ href, navigate }"
          :to="item.to"
          custom
        >
          <a
            :href="href"
            :aria-current="route.meta.tab === item.tab ? 'page' : undefined"
            :class="
              cn(
                'flex flex-col items-center justify-center gap-0.5 text-xs text-muted-foreground',
                route.meta.tab === item.tab && 'font-medium text-foreground',
              )
            "
            @click="navigate"
          >
            <component :is="item.icon" class="size-5" aria-hidden="true" />
            {{ item.label }}
          </a>
        </RouterLink>
      </nav>
    </div>
  </div>
</template>
