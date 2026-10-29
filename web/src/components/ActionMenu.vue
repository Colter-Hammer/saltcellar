<script setup lang="ts">
// A ⋯ button opening a list of actions. An action with `choices` opens a sub-list; a disabled one
// shows why in its label.
import { ChevronRight, Ellipsis } from '@lucide/vue'
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuPortal,
  DropdownMenuRoot,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from 'reka-ui'

export interface Action {
  label: string
  run?: () => void
  disabled?: boolean
  choices?: { label: string; run: () => void }[]
}

defineProps<{
  /** What the button is called for screen readers, e.g. "Groceries actions". */
  label: string
  actions: Action[]
}>()

const item =
  'cursor-pointer rounded px-2 py-1.5 text-sm outline-none data-[disabled]:cursor-default data-[disabled]:text-muted-foreground data-[highlighted]:bg-accent'
</script>

<template>
  <DropdownMenuRoot :modal="false">
    <DropdownMenuTrigger
      :aria-label="label"
      class="rounded-md p-1.5 text-muted-foreground hover:bg-accent"
    >
      <Ellipsis class="size-4" aria-hidden="true" />
    </DropdownMenuTrigger>
    <DropdownMenuPortal>
      <DropdownMenuContent
        align="end"
        class="z-40 min-w-44 rounded-md border bg-background p-1 shadow-md"
      >
        <template v-for="action in actions" :key="action.label">
          <DropdownMenuSub v-if="action.choices">
            <DropdownMenuSubTrigger
              :disabled="action.disabled ?? false"
              :class="[item, 'flex items-center']"
            >
              {{ action.label }}
              <ChevronRight class="ml-auto size-4" aria-hidden="true" />
            </DropdownMenuSubTrigger>
            <DropdownMenuPortal>
              <DropdownMenuSubContent
                class="z-40 max-h-72 min-w-40 overflow-y-auto rounded-md border bg-background p-1 shadow-md"
              >
                <DropdownMenuItem
                  v-for="choice in action.choices"
                  :key="choice.label"
                  :class="item"
                  @select="choice.run"
                >
                  {{ choice.label }}
                </DropdownMenuItem>
              </DropdownMenuSubContent>
            </DropdownMenuPortal>
          </DropdownMenuSub>
          <DropdownMenuItem
            v-else
            :disabled="action.disabled ?? false"
            :class="item"
            @select="action.run?.()"
          >
            {{ action.label }}
          </DropdownMenuItem>
        </template>
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>
