[
  .files[] as $file
  | ($file.new_path.display // $file.old_path.display) as $path
  | $file.hunks[]
  | (
      if ($path | endswith("bun.lock")) then
        {score: 5, tags: ["mechanical"], rationale: "Generated dependency lock change"}
      elif ($path | endswith("package.json")) then
        {score: 30, tags: ["configuration"], rationale: "Dependency and command configuration change"}
      elif ($path | endswith("schema.prisma")) then
        {score: 88, tags: ["configuration", "behavior"], rationale: "Database provider contract changes runtime persistence behavior"}
      elif ($path | contains("device-api.controller.ts")) then
        {score: 98, tags: ["security", "api", "non-trivial-logic"], rationale: "New device-facing API boundary contains authentication and request behavior"}
      elif ($path | contains("device-runtime.module.ts")) then
        {score: 91, tags: ["security", "configuration", "behavior"], rationale: "New runtime composition controls exposed device capabilities"}
      elif ($path | endswith("main.device.ts")) then
        {score: 89, tags: ["configuration", "behavior"], rationale: "New application bootstrap selects the device runtime and global policy"}
      elif ($path | endswith("prisma.service.ts")) then
        {score: 94, tags: ["configuration", "non-trivial-logic", "behavior"], rationale: "Database adapter initialization and lifecycle affect all persistence operations"}
      elif ($path | endswith("bmp.util.ts")) then
        {score: 90, tags: ["non-trivial-logic", "behavior"], rationale: "Binary image encoding logic changes device output bytes"}
      elif ($path | endswith("sharp.ts")) then
        {score: 84, tags: ["behavior", "api"], rationale: "Compatibility wrapper changes image processing dependency semantics across callers"}
      elif ($path | contains("screen-renderer.takumi.service.ts")) and ($path | endswith(".test.ts") | not) then
        {score: 93, tags: ["non-trivial-logic", "behavior", "api"], rationale: "New renderer implements native loading, fonts, raster conversion, and fallback behavior"}
      elif ($path | contains("screen-renderer.satori.service.ts")) and ($path | endswith(".test.ts") | not) then
        {score: 89, tags: ["non-trivial-logic", "behavior"], rationale: "Renderer rewrite changes parsing, fonts, rasterization, and device image output"}
      elif ($path | contains("screen-renderer.service.ts")) and ($path | endswith(".test.ts") | not) then
        {score: 82, tags: ["non-trivial-logic", "behavior"], rationale: "Shared renderer behavior and fallback paths affect all screen output"}
      elif ($path | contains("screen-composer.service.ts")) and ($path | endswith(".test.ts") | not) then
        {score: 80, tags: ["non-trivial-logic", "behavior"], rationale: "Composition changes alter the document passed to every renderer"}
      elif ($path | contains("framework-jsx-executor")) then
        {score: 77, tags: ["security", "non-trivial-logic"], rationale: "Dynamic JSX execution boundary changes accepted framework output"}
      elif ($path | endswith(".test.ts")) or ($path | endswith(".spec.ts")) then
        {score: 48, tags: ["tests"], rationale: "Test changes provide evidence for related production behavior"}
      elif ($path | endswith("weather-widget.service.tsx")) then
        {score: 58, tags: ["behavior"], rationale: "User-visible rendering behavior changes across multiple conditions"}
      elif ($path | endswith(".d.ts")) then
        {score: 52, tags: ["api", "configuration"], rationale: "Type declaration defines the imported package contract"}
      elif (.patch | length) > 3000 then
        {score: 75, tags: ["non-trivial-logic", "behavior"], rationale: "Large production logic change needs early semantic review"}
      elif ($path | endswith(".ts")) or ($path | endswith(".tsx")) then
        {score: 42, tags: ["behavior"], rationale: "Production source change with localized behavioral impact"}
      else
        {score: 20, tags: ["mechanical"], rationale: "Low-risk supporting change"}
      end
    ) as $assessment
  | {
      node_id: .id,
      score: $assessment.score,
      tags: $assessment.tags,
      rationale: $assessment.rationale,
      confidence: 0.82,
      evidence_ids: [.id],
      authority: "model"
    }
]
