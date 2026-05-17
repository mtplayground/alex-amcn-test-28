import { ArrowUpRight, Orbit, Sparkles } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

const highlights = [
  {
    title: "Vite + React + TypeScript",
    description: "Fast local iteration with a typed SPA scaffold ready for the graph explorer.",
  },
  {
    title: "Tailwind + shadcn-style primitives",
    description: "Theme tokens and base components are in place for the next UI issues.",
  },
  {
    title: "Backend dev proxy",
    description: "Requests to /api are forwarded to the Axum server on port 8080 during local development.",
  },
];

export default function App() {
  return (
    <main className="min-h-screen bg-background text-foreground">
      <div className="absolute inset-0 -z-10 bg-[radial-gradient(circle_at_top_left,rgba(14,165,233,0.18),transparent_32%),radial-gradient(circle_at_bottom_right,rgba(249,115,22,0.18),transparent_26%)]" />
      <div className="absolute inset-0 -z-10 bg-hero-grid bg-[size:40px_40px] [mask-image:linear-gradient(to_bottom,white,transparent)] opacity-40" />

      <section className="mx-auto flex min-h-screen w-full max-w-6xl flex-col justify-center gap-10 px-6 py-16 sm:px-10">
        <div className="flex flex-col gap-6">
          <div className="inline-flex w-fit items-center gap-2 rounded-full border border-border/70 bg-card/80 px-3 py-1 text-sm font-medium text-muted-foreground shadow-sm backdrop-blur">
            <Orbit className="h-4 w-4 text-primary" />
            ZeroClaw frontend scaffold
          </div>

          <div className="max-w-3xl space-y-4">
            <h1 className="text-balance text-5xl font-semibold tracking-tight sm:text-6xl">
              Hello from the React workspace.
            </h1>
            <p className="max-w-2xl text-lg leading-8 text-muted-foreground">
              The Vite, Tailwind, and shadcn-style UI foundation is ready. Future issues can build the
              graph canvas, query editor, and results panels on top of this shell.
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <Button className="gap-2">
              Launch next UI slice
              <ArrowUpRight className="h-4 w-4" />
            </Button>
            <Button variant="outline" className="gap-2">
              <Sparkles className="h-4 w-4" />
              Tailwind tokens loaded
            </Button>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          {highlights.map((item) => (
            <Card key={item.title} className="border-border/70 bg-card/85 backdrop-blur">
              <CardHeader>
                <CardTitle>{item.title}</CardTitle>
                <CardDescription>{item.description}</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="h-1 w-16 rounded-full bg-gradient-to-r from-sky-500 via-cyan-400 to-amber-400" />
              </CardContent>
            </Card>
          ))}
        </div>
      </section>
    </main>
  );
}
