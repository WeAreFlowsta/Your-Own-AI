import { component$ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { useVisibleTask$ } from "@builder.io/qwik";

/**
 * Index route — redirects to /chat.
 * Uses client-side navigation so it works with the static adapter.
 */
export default component$(() => {
  const nav = useNavigate();

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    nav("/chat/");
  });

  return null;
});

export const head: DocumentHead = {
  title: "Your Own AI",
};
