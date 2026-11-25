// Builds the McpServer with the Kokon tools. Shared by the two entry points: stdio
// (index.ts — the MCP client starts the process) and `serve` (serve.ts — Streamable HTTP,
// for HTTP clients such as Open WebUI). The descriptions are deliberately model-agnostic:
// the box decides which image model is the default (QLS_MCP_MODEL can pin one — see
// ServiceClient.alignModel), and get_generator_status reports which one is running.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { handlers, schemas, toErrorResult, type ToolContext, type ToolResult } from "./tools.js";
import { ServiceClient } from "./service.js";
import pkg from "../package.json" with { type: "json" };

// Bundled in by esbuild: the one place to bump is package.json (see docs/releasing.md).
export const VERSION = pkg.version;

export const DESCRIPTIONS: Record<keyof typeof schemas, { title: string; description: string }> = {
  generate_image: {
    title: "Generate image",
    description:
      "Generate an image from a text prompt with the box's default image model, optionally using reference images (a person, a product, a style) in a new scene. Write the full, detailed prompt yourself (see the prompt field): it is used as is. Dimensions are chosen automatically (aspect ratio can be inferred from the prompt, e.g. '16:9 hero image') unless width/height or aspect_ratio are given. Returns the absolute path of the saved PNG. Runs fully offline; typically 45 s–4 min.",
  },
  edit_image: {
    title: "Edit image",
    description:
      "Edit an existing image with a natural-language instruction (e.g. 'make the background nighttime, keep the foreground unchanged'). Preserves the input's width, height and aspect ratio by default. The input file is never overwritten.",
  },
  edit_images: {
    title: "Edit with multiple references",
    description:
      "Edit image 1 with the help of other images (up to 10 in all) and an instruction, e.g. put the product from image 2 in the hand of the person in image 1. Order matters: refer to them as image 1, image 2, ... in the prompt. The output follows image 1's aspect ratio unless width/height or aspect_ratio are given. For a new scene that only uses the images as references, use generate_image with reference_images.",
  },
  outpaint_image: {
    title: "Expand image (outpaint)",
    description:
      "Extend an image beyond its borders. Give the expansion per side in pixels or percent; the canvas, mask and model-aligned sizes are built automatically and the original pixels are preserved exactly.",
  },
  inpaint_image: {
    title: "Inpaint image",
    description:
      "Change only the masked area of an image (mask: white = change, black = keep): edit it, remove what is there, replace it, or add something inside it. The model is shown the area marked on a copy of the image; pixels outside the mask are preserved exactly.",
  },
  upscale_image: {
    title: "Upscale image",
    description:
      "Enlarge an image (default 2x, or to a width/height up to 8192 px). Runs the upscaler model chosen in Kokon (Superscale-SP by default; seconds on a PC GPU, longer on a Mac; adds no new content). Saves a new file; the input is never overwritten.",
  },
  get_image_info: {
    title: "Image info",
    description: "Width, height, aspect ratio, file type and, for images made by Kokon, the generation metadata (prompt, seed, model, steps...).",
  },
  get_generator_status: {
    title: "Generator status",
    description: "Whether the local inference server is running and the model loaded, GPU and VRAM, which model the box is using, the active job and the queue.",
  },
};

export function makeServer(client: ServiceClient, name = "kokon"): McpServer {
  const server = new McpServer({ name, version: VERSION }, { capabilities: { tools: {} } });

  function toMcp(r: ToolResult) {
    const content: ({ type: "text"; text: string } | { type: "image"; data: string; mimeType: string })[] = [{ type: "text", text: r.text }];
    if (r.previewJpeg) content.push({ type: "image", data: r.previewJpeg.toString("base64"), mimeType: "image/jpeg" });
    return { content, structuredContent: r.structured, isError: r.isError ?? false };
  }

  for (const tool of Object.keys(schemas) as (keyof typeof schemas)[]) {
    const d = DESCRIPTIONS[tool];
    server.registerTool(
      tool,
      {
        title: d.title,
        description: d.description,
        inputSchema: schemas[tool],
        annotations: { readOnlyHint: tool.startsWith("get_"), openWorldHint: false, destructiveHint: false, idempotentHint: tool.startsWith("get_") },
      },
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      async (args: any, extra: any) => {
        const token = extra?._meta?.progressToken;
        let n = 0;
        const ctx: ToolContext = {
          client,
          // Claude Code sets CLAUDE_PROJECT_DIR for MCP servers; otherwise use the working directory.
          cwd: process.env.CLAUDE_PROJECT_DIR || process.cwd(),
          signal: extra?.signal,
          progress: (message) => {
            if (token === undefined) return;
            n += 1;
            extra.sendNotification({ method: "notifications/progress", params: { progressToken: token, progress: n, message } }).catch(() => {});
          },
        };
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          return toMcp(await (handlers[tool] as any)(args, ctx));
        } catch (e) {
          return toMcp(toErrorResult(e));
        }
      },
    );
  }
  return server;
}
