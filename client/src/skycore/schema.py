"""IR models.

Targets are validated strictly because the matcher depends on their fields.
Steps are kept as plain dicts: the server owns strict step validation (zod), and
the interpreter dispatches on ``op`` anyway. Duplicating a recursive step schema
in two languages would mean two places to update and two chances to disagree.
"""

from __future__ import annotations

from typing import Annotated, Any, Literal, Union

from pydantic import BaseModel, ConfigDict, Field

NormTuple = tuple[float, float]
NormRect = tuple[float, float, float, float]


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="allow", populate_by_name=True)


class AssetBox(StrictModel):
    w: float = Field(gt=0)
    h: float = Field(gt=0)


class ScaleSpec(StrictModel):
    mode: Literal["auto", "fixed"] = "auto"
    # Wide enough to cover "fixed size UI on a bigger screen" (0.7x) as well as
    # proportional layouts that come out slightly off (1.5x).
    min: float = 0.7
    max: float = 1.5
    steps: int = 9


class TemplateTarget(StrictModel):
    type: Literal["template"]
    asset: str
    assetBox: AssetBox
    recordScale: float = 1.0
    """
    Logical width of the capture surface the crop was taken from.

    This is what makes one crop work on a differently sized screen: the template
    is normalized by the *recording* surface width, exactly like the live frame
    is normalized by the current one, so a control that kept its relative size
    matches without any scale search. Falls back to the runtime width when a
    script predates this field.
    """

    recordWidth: float = 0.0
    roi: NormRect
    anchor: NormTuple = (0.5, 0.5)
    strategy: list[Literal["tpl", "mstpl", "sift"]] = ["tpl", "mstpl"]
    scale: ScaleSpec = ScaleSpec()
    threshold: float = 0.82
    warnThreshold: float = 0.7
    verify: Literal["none", "rgb"] = "rgb"
    mask: Literal["none", "edges"] = "none"
    multi: Literal["nearest-last", "highest", "center", "nth"] = "nearest-last"
    nth: int | None = None
    auto: bool = True


class PointTarget(StrictModel):
    type: Literal["point"]
    norm: NormTuple
    zone: NormRect


Target = Annotated[Union[TemplateTarget, PointTarget], Field(discriminator="type")]


class IRMeta(StrictModel):
    name: str
    game: str | None = None
    minEngine: str | None = None
    maxDurationMs: int = 1_800_000


class IRDoc(StrictModel):
    irVersion: int = 1
    meta: IRMeta
    requires: list[str] = ["screenshot", "templateMatch", "gesture"]
    entry: str = "main"
    vars: dict[str, str | float | bool] = Field(default_factory=dict)
    blocks: dict[str, list[dict[str, Any]]]
    targets: dict[str, Target]

    def target(self, name: str) -> TemplateTarget | PointTarget | None:
        return self.targets.get(name)

    def template_target(self, name: str) -> TemplateTarget | None:
        target = self.targets.get(name)
        return target if isinstance(target, TemplateTarget) else None

    def steps(self, block: str) -> list[dict[str, Any]]:
        return self.blocks.get(block, [])

    def validate(self) -> list[str]:
        """Structural checks the schema cannot express, mirroring the server."""
        problems: list[str] = []
        if self.entry not in self.blocks:
            problems.append(f'entry block "{self.entry}" does not exist')

        referenced: set[str] = set()

        def walk(steps: list[dict[str, Any]]) -> None:
            for step in steps:
                for key in ("target", "from", "to"):
                    value = step.get(key)
                    if isinstance(value, str):
                        referenced.add(value)
                if isinstance(step.get("any"), list):
                    referenced.update(name for name in step["any"] if isinstance(name, str))
                when = step.get("when")
                if isinstance(when, dict):
                    collect_conditions(when, referenced)
                for key in ("body", "then", "else"):
                    nested = step.get(key)
                    if isinstance(nested, list):
                        walk(nested)

        for block in self.blocks.values():
            walk(block)

        for name in sorted(referenced):
            if name not in self.targets:
                problems.append(f'target "{name}" is referenced but not defined')

        for name, target in self.targets.items():
            if isinstance(target, TemplateTarget) and not target.asset:
                problems.append(f'target "{name}" has no asset')

        return problems

    def asset_hashes(self) -> list[str]:
        return [t.asset for t in self.targets.values() if isinstance(t, TemplateTarget)]


def collect_conditions(condition: dict[str, Any], out: set[str]) -> None:
    visible = condition.get("visible")
    if isinstance(visible, str):
        out.add(visible)
    elif isinstance(visible, list):
        out.update(name for name in visible if isinstance(name, str))

    missing = condition.get("missing")
    if isinstance(missing, str):
        out.add(missing)

    for key in ("all", "any"):
        value = condition.get(key)
        if isinstance(value, list):
            for item in value:
                if isinstance(item, dict):
                    collect_conditions(item, out)

    nested = condition.get("not")
    if isinstance(nested, dict):
        collect_conditions(nested, out)
