"""Poses the hands on the guns and writes them to assets/models/<gun>_hands.obj.

The arms are the CC0 "WRAD ARMS" by wriks (arms.blend, LICENSE.txt). The guns are modelled in
code (src/viewmodel), so they are exported first; from the repository's root:

    cargo test export_guns -- --ignored
    blender -b tools/hands/arms.blend --python tools/hands/pose.py -- [--look DIR]

With --look, pictures of each gun in its hands are rendered to DIR: `<gun>_eye.png` as the
player sees it at 16:9, and close-ups around each hand.

How a hand is put on a gun (`HOLDS`): the base knuckle of its middle finger goes to a place on
the gun, the palm faces one way and the thumb's side of the hand another, and the forearm comes
from a place below the screen. The fingers then close until they touch the gun; the trigger
finger and the thumb are laid where they are told to go. Everything here is in the gun's space as the game has it: metres, X right, Y up,
-Z forward. What is printed says how each finger ended up and what of a hand is sunk into the
gun; `cargo test viewmodel` then checks the hands like the guns (closed, facing out, in their
corner of the screen).
"""

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

ROOT = Path(__file__).resolve().parents[2]
EXPORTED = ROOT / "target" / "hands"
MODELS = ROOT / "assets" / "models"

# Wrist to the tip of the middle finger (m): a large hand in a glove.
HAND_LENGTH = 0.19
# Half the thickness of a finger at its three bones (m): how near their middles come to the gun.
FINGER_RADIUS = (0.0085, 0.0075, 0.0065)
# The glove's harder parts: a plate on the back of the hand from here to here along it, and a
# cuff this far up the forearm (m).
PLATE = (0.030, 0.090)
CUFF = 0.035
# Shading of the hands' faces (see `shade`): rays per face, how far they look (m), and the
# share of its finish a face with nothing open before it keeps.
SKY_RAYS = 64
SKY_REACH = 0.035
DARKEST = 0.45
# How the trigger finger and the thumbs are laid on the gun (see `Hand.reach`), for a finger and
# for a thumb: how far its base and its joints turn (degrees), how far the middle of each of its
# three bones stays from the gun's surface (m), which of the bones that goes for, and the turns
# the search starts from (the base's bend, spread and twist, then the two joints' bends). A
# finger passes close by a trigger guard; a thumb's first bone is inside the ball of the thumb,
# which presses on what it holds.
REACH = {
    "finger": (
        {"bend": (-15.0, 80.0), "spread": (-25.0, 25.0), "twist": (0.0, 0.0), "joints": (0.0, 100.0)},
        (0.0060, 0.0055, 0.0050),
        (1, 2),
        (10.0, 0.0, 0.0, 40.0, 30.0),
    ),
    "thumb": (
        {"bend": (-60.0, 80.0), "spread": (-70.0, 70.0), "twist": (-40.0, 40.0), "joints": (-10.0, 70.0)},
        (0.0, 0.0085, 0.0075),
        (1, 2),
        (0.0, 0.0, 0.0, 10.0, 10.0),
    ),
}
FINGERS = ("index", "middle", "ring", "pinky")
# How far each joint of a finger bends at most (degrees from straight).
LIMITS = (90.0, 110.0, 75.0)
STEP = 2.0
# The bends a finger starts closing from: curled enough to reach round a grip, not into what is
# ahead of it (a magazine, a trigger guard).
OPEN = (20.0, 40.0, 20.0)


def at(forward, up, right):
    """The point `forward`, `up` and `right` of a gun's origin (see `at` in mesh.rs)."""
    return Vector((right, up, -forward))


# Where a trigger finger's pad and tip go: in front of the trigger blade, across the gun.
def on_trigger(forward, up):
    return dict(pad=at(forward, up, 0.0), tip=at(forward - 0.002, up, -0.011))


# Where a thumb goes that lies along the gun's left side, pointing forward: its tip, and its last
# joint behind that.
def along_left(forward, up, half_width):
    return dict(tip=at(forward, up, -half_width - 0.0075), joint=at(forward - 0.028, up, -half_width - 0.008))


# Per gun, its hands. `side`: which arm. `knuckle`: where the joint at the base of the middle
# finger goes (the skin of the palm is about 12 mm from it).
# `palm`: the way the palm faces. `thumbward`: the way the hand's thumb side faces. `elbow`:
# where the forearm comes from, in the eye's space (X right, Y up, -Z ahead). `index` and `thumb`:
# where those are laid (see `Hand.reach`); without `index` that finger closes round the gun like
# the other three.
HOLDS = {
    "pistol": [
        dict(
            side="r",
            knuckle=at(-0.0095, -0.054, 0.0263),
            palm=at(0.1, 0.0, -1.0),
            thumbward=at(-0.075, 1.0, 0.0),
            elbow=Vector((0.34, -0.42, -0.02)),
            # On the trigger, and along the frame under the slide, behind the slide stop.
            index=on_trigger(0.026, -0.026),
            thumb=along_left(0.000, -0.004, 0.0136),
        ),
    ],
    "rifle": [
        dict(
            side="r",
            knuckle=at(-0.0183, -0.057, 0.0261),
            palm=at(0.1, 0.0, -1.0),
            thumbward=at(0.0, 1.0, 0.0),
            elbow=Vector((0.30, -0.45, 0.02)),
            # On the trigger, and round the grip under the lower receiver.
            index=on_trigger(0.036, -0.032),
            thumb=along_left(-0.006, -0.030, 0.0141),
        ),
        dict(
            side="l",
            knuckle=at(0.212, -0.018, 0.027),
            palm=at(0.0, 1.0, 0.45),
            thumbward=at(1.0, 0.0, -0.36),
            elbow=Vector((0.15, -0.55, -0.18)),
            # Up the handguard's side.
            thumb=dict(tip=at(0.252, 0.042, -0.026), joint=at(0.244, 0.015, -0.027)),
        ),
    ],
}

arms = bpy.data.objects["arms"]
skin = bpy.data.objects["arms_mesh"]
bones = arms.data.bones
pose = arms.pose.bones


def update():
    bpy.context.view_layer.update()


def prepare():
    """Frees the arms from the rig's helpers (each bone is placed directly here) and puts the
    armature into the gun's space at the hand's size: its Y is forward and its Z up."""
    bpy.context.view_layer.objects.active = arms
    bpy.ops.object.mode_set(mode="EDIT")
    for bone in arms.data.edit_bones:
        if bone.name.startswith("bicep"):
            bone.use_connect = False
    bpy.ops.object.mode_set(mode="OBJECT")
    for bone in pose:
        for constraint in list(bone.constraints):
            if constraint.type == "IK" or bone.name.startswith("wrist"):
                bone.constraints.remove(constraint)
    rest_length = (bones["finger_middle3.r"].tail_local - bones["wrist.r"].head_local).length
    scale = HAND_LENGTH / rest_length
    arms.matrix_world = Matrix(((scale, 0, 0, 0), (0, 0, scale, 0), (0, -scale, 0, 0), (0, 0, 0, 1)))
    update()


def frame(wrist, along, thumbward, palm):
    """A hand's frame as a matrix: columns along the hand, to its thumb's side, out of its palm."""
    matrix = Matrix.Identity(4)
    for column, axis in enumerate((along, thumbward, palm)):
        for row in range(3):
            matrix[row][column] = axis[row]
    matrix.translation = wrist
    return matrix


def palm_of(side, along, thumbward):
    return thumbward.cross(along) if side == "r" else along.cross(thumbward)


def rest_frame(side):
    wrist = bones[f"wrist.{side}"].head_local
    along = (bones[f"finger_middle1.{side}"].head_local - wrist).normalized()
    across = bones[f"finger_index1.{side}"].head_local - bones[f"finger_pinky1.{side}"].head_local
    thumbward = (across - along * across.dot(along)).normalized()
    return frame(wrist, along, thumbward, palm_of(side, along, thumbward))


class Hand:
    """One arm being posed. Points and directions are the armature's until `world` is applied."""

    def __init__(self, hold, gun, eye):
        self.side = side = hold["side"]
        self.hold = hold
        self.gun = gun
        self.to_armature = arms.matrix_world.inverted()
        self.sign = 1.0 if side == "r" else -1.0
        rest = rest_frame(side)
        # The frame asked for, in the armature.
        turn = self.to_armature.to_3x3()
        palm = (turn @ hold["palm"]).normalized()
        thumbward = turn @ hold["thumbward"]
        thumbward = (thumbward - palm * thumbward.dot(palm)).normalized()
        along = palm.cross(thumbward) if side == "r" else thumbward.cross(palm)
        # The wrist that puts the knuckle on its place.
        knuckle = rest.inverted() @ bones[f"finger_middle1.{side}"].head_local
        target = self.to_armature @ hold["knuckle"]
        wrist = target - (along * knuckle.x + thumbward * knuckle.y + palm * knuckle.z)
        self.frame = frame(wrist, along, thumbward, palm)
        self.move = self.frame @ rest.inverted()
        self.scale = arms.matrix_world.to_scale().x
        elbow = self.to_armature @ (eye.inverted() @ hold["elbow"])
        self.place_arm(wrist, elbow)

    def bone(self, name):
        return pose[f"{name}.{self.side}"]

    def axis(self, name):
        return self.frame.col[("along", "thumbward", "palm").index(name)].to_3d()

    def place_arm(self, wrist, elbow):
        side = self.side
        start = self.move @ bones[f"forearm.{side}"].head_local
        swing = (start - wrist).rotation_difference(elbow - wrist).to_matrix().to_4x4()
        forearm = Matrix.Translation(wrist) @ swing @ Matrix.Translation(-wrist) @ self.move
        self.bone("bicep").matrix = forearm @ bones[f"bicep.{side}"].matrix_local
        update()
        self.bone("wrist").matrix = self.move @ bones[f"wrist.{side}"].matrix_local
        update()
        off = (self.bone("wrist").head - wrist).length / self.scale
        assert off < 1e-4, f"the wrist is {off} m off"

    def turn(self, bone, axis, degrees):
        """Turns a bone about an axis through its head; the left hand mirrors the right."""
        head = bone.head.copy()
        spin = Matrix.Rotation(math.radians(degrees * self.sign), 4, axis)
        bone.matrix = Matrix.Translation(head) @ spin @ Matrix.Translation(-head) @ bone.matrix
        update()

    def bend(self, finger, joint):
        """How far a finger's joint (0 to 2) is bent, in degrees from straight."""
        chain = [self.bone(f"finger_{finger}{k}") for k in (1, 2, 3)]
        direction = lambda bone: (bone.tail - bone.head).normalized()
        before = self.axis("along") if joint == 0 else direction(chain[joint - 1])
        after = direction(chain[joint])
        axis = self.axis("thumbward") * self.sign
        return math.degrees(math.atan2(before.cross(after).dot(axis), before.dot(after)))

    def set_bend(self, finger, joint, degrees):
        bone = self.bone(f"finger_{finger}{joint + 1}")
        self.turn(bone, self.axis("thumbward"), degrees - self.bend(finger, joint))

    def touches(self, finger, joint):
        """Whether the finger from `joint` outwards touches the gun. The first bone's inner part
        doesn't count: it turns in place next to the palm."""
        world = arms.matrix_world
        for k in range(joint, 3):
            bone = self.bone(f"finger_{finger}{k + 1}")
            for step in range(2 if k == joint else 0, 7):
                point = world @ bone.head.lerp(bone.tail, step / 6)
                if self.gun.find_nearest(point)[3] < FINGER_RADIUS[k] or inside(self.gun, point):
                    return True
        return False

    def close(self, finger):
        """Half opens a finger, then bends its joints together, each until what it moves touches
        the gun, like a hand closing round something."""
        bends = list(OPEN)
        for joint in range(3):
            self.set_bend(finger, joint, bends[joint])
        assert not self.touches(finger, 0), f"the open {finger} finger is in the gun"
        closing = [0, 1, 2]
        while closing:
            for joint in list(closing):
                further = bends[joint] + STEP * LIMITS[joint] / LIMITS[0]
                if further <= LIMITS[joint]:
                    self.set_bend(finger, joint, further)
                    if not self.touches(finger, joint):
                        bends[joint] = further
                        continue
                    self.set_bend(finger, joint, bends[joint])
                closing.remove(joint)
        return bends

    def reach(self, digit, goal):
        """Lays a digit (the trigger finger, a thumb) where `goal` says: the places in the gun's
        space of its `tip`, of the `pad` in the middle of its last bone, or of its last `joint`.
        Its base turns every way a knuckle can and the two joints beyond it only bend, within
        `REACH[digit]`. The pose is found by trying turns of each and keeping those that bring
        the digit nearer its goal, where being in the gun, or nearer its surface than the digit
        is thick, counts against a pose. Returns the turns in degrees and how far off the goal
        is left, in millimetres."""
        limits, thickness, guarded, start = REACH["thumb" if digit == "thumb" else "finger"]
        chain = [self.bone(f"finger_{digit}{k}") for k in (1, 2, 3)]
        world = arms.matrix_world
        for bone in chain:
            bone.matrix_basis = Matrix.Identity(4)
        update()
        # A finger curls towards the palm, a thumb towards the palm and the fingers' side. Each
        # bone bends about the axis square to itself and to that direction; the base also
        # spreads (about the axis square to both) and twists (about itself). The axes are kept
        # in each bone's own space, where they stay the same however the bones before it turn.
        curl = self.axis("palm") - (self.axis("thumbward") if digit == "thumb" else Vector())
        own = []
        for bone in chain:
            along = (bone.tail - bone.head).normalized()
            bend = along.cross(curl).normalized()
            to_own = bone.matrix.to_3x3().inverted()
            own.append([(to_own @ axis).normalized() for axis in (bend, along.cross(bend), along)])
        # What may turn: (bone, which of its axes, least, most).
        turns = [
            (0, 0, *limits["bend"]),
            (0, 1, *limits["spread"]),
            (0, 2, *limits["twist"]),
            (1, 0, *limits["joints"]),
            (2, 0, *limits["joints"]),
        ]

        def place(angles):
            bases = [Matrix.Identity(4) for _ in chain]
            for (k, axis, _, _), angle in zip(turns, angles):
                bases[k] = bases[k] @ Matrix.Rotation(math.radians(angle), 4, own[k][axis])
            for bone, basis in zip(chain, bases):
                bone.matrix_basis = basis
            update()

        def off(angles):
            place(angles)
            last = chain[2]
            at = {"tip": last.tail, "joint": last.head, "pad": last.head.lerp(last.tail, 0.5)}
            cost = sum((world @ at[name] - place_).length_squared * 1e6 for name, place_ in goal.items())
            for k in guarded:
                bone = chain[k]
                for step in range(1, 6):
                    point = world @ bone.head.lerp(bone.tail, step / 5)
                    depth = self.gun.find_nearest(point)[3] * (-1.0 if inside(self.gun, point) else 1.0)
                    cost += 3.0 * max(0.0, thickness[k] - depth) ** 2 * 1e6
            # Of two poses that are as good, the more relaxed one.
            return cost + 0.002 * sum(angle * angle for angle in angles)

        angles = list(start)
        best = off(angles)
        for step in (20.0, 10.0, 5.0, 2.5, 1.0):
            better = True
            while better:
                better = False
                for i, (_, _, least, most) in enumerate(turns):
                    for change in (step, -step):
                        tried = list(angles)
                        tried[i] = min(most, max(least, tried[i] + change))
                        if tried[i] != angles[i] and (cost := off(tried)) < best - 1e-6:
                            angles, best, better = tried, cost, True
                            break
        place(angles)
        last = chain[2]
        at = {"tip": last.tail, "joint": last.head, "pad": last.head.lerp(last.tail, 0.5)}
        left = {name: round((world @ at[name] - place_).length * 1000, 1) for name, place_ in goal.items()}
        return [round(angle) for angle in angles], left


def inside(gun, point):
    """Whether a point is inside the gun, which is closed shells that may overlap: a ray from
    inside a shell leaves it once more than it enters it."""
    ray = Vector((0.31, 0.77, 0.55)).normalized()
    leaves = 0
    while True:
        place, normal, _, _ = gun.ray_cast(point, ray)
        if place is None:
            return leaves > 0
        leaves += 1 if normal.dot(ray) > 0.0 else -1
        point = place + ray * 1e-6


def read_gun(name):
    places, colors, faces = [], [], []
    for line in (EXPORTED / f"{name}.obj").read_text().splitlines():
        words = line.split()
        if words[0] == "v":
            places.append(Vector(float(word) for word in words[1:4]))
            colors.append(tuple(float(word) for word in words[4:7]))
        elif words[0] == "f":
            faces.append(tuple(int(word) - 1 for word in words[1:]))
    held = json.loads((EXPORTED / f"{name}.json").read_text())
    rest = held["rest"]
    eye = Matrix([[rest[column * 4 + row] for column in range(4)] for row in range(4)])
    return places, colors, faces, eye, held


def shell(side):
    """The posed forearm and hand of one side: places in the gun's space and faces with their
    finishes, cut off at the elbow and closed there."""
    graph = bpy.context.evaluated_depsgraph_get()
    posed = skin.evaluated_get(graph)
    mesh = posed.to_mesh()
    names = [group.name for group in skin.vertex_groups]
    weights = [{names[group.group]: group.weight for group in vertex.groups} for vertex in mesh.vertices]
    upper = lambda vertex: sum(weight for name, weight in weights[vertex].items() if not name.startswith(("forearm", "wrist", "finger")))
    ours = lambda vertex: max(weights[vertex], key=weights[vertex].get).endswith("." + side)
    lower = lambda vertex: sum(weight for name, weight in weights[vertex].items() if name.startswith("forearm"))
    places = [posed.matrix_world @ vertex.co for vertex in mesh.vertices]
    # Where each corner is on the hand at rest (m): along it from the wrist, to the thumb's side,
    # out of the palm. The glove's plate and cuff are laid out in these.
    to_hand = rest_frame(side).inverted()
    scale = arms.matrix_world.to_scale().x
    on_hand = [to_hand @ vertex.co * scale for vertex in skin.data.vertices]
    faces = []
    for polygon in mesh.polygons:
        corners = list(polygon.vertices)
        if not all(ours(corner) for corner in corners) or max(upper(corner) for corner in corners) > 0.5:
            continue
        middle = sum((on_hand[corner] for corner in corners), Vector()) / len(corners)
        if sum(lower(corner) for corner in corners) / len(corners) > 0.5:
            finish = "pad" if middle.x > -CUFF else "sleeve"
        elif PLATE[0] < middle.x < PLATE[1] and middle.z < 0.0 and all(on_hand[corner].z < 0.004 for corner in corners):
            finish = "pad"
        else:
            finish = "glove"
        faces.append((finish, corners))
    # Close the cut: its edges are the ones a single face uses.
    edges = {}
    for _, corners in faces:
        for start, end in zip(corners, corners[1:] + corners[:1]):
            edges[(start, end)] = edges.get((start, end), 0) + 1
    rim = [(start, end) for (start, end) in edges if (end, start) not in edges]
    assert rim, "nothing was cut"
    centre = sum((places[start] for start, _ in rim), Vector()) / len(rim)
    places.append(centre)
    for start, end in rim:
        faces.append(("sleeve", [end, start, len(places) - 1]))
    posed.to_mesh_clear()
    # Only what is used, and bent quads as two triangles so every face is flat enough to shade.
    used = sorted({corner for _, corners in faces for corner in corners})
    renumber = {old: new for new, old in enumerate(used)}
    flat = []
    for finish, corners in faces:
        corners = [renumber[corner] for corner in corners]
        if len(corners) == 4:
            a, b, c, d = (places[used[corner]] for corner in corners)
            first, second = (b - a).cross(c - a), (c - a).cross(d - a)
            other_first, other_second = (c - b).cross(d - b), (d - b).cross(a - b)
            if first.normalized().dot(second.normalized()) < 0.94:
                # Split along the diagonal that leaves the flatter pair.
                if other_first.normalized().dot(other_second.normalized()) > first.normalized().dot(second.normalized()):
                    flat += [(finish, [corners[1], corners[2], corners[3]]), (finish, [corners[3], corners[0], corners[1]])]
                else:
                    flat += [(finish, corners[:3]), (finish, [corners[0], corners[2], corners[3]])]
                continue
        elif len(corners) > 4:
            flat += [(finish, [corners[0], corners[i], corners[i + 1]]) for i in range(1, len(corners) - 1)]
            continue
        flat.append((finish, corners))
    parts = [max(weights[old], key=weights[old].get)[:-2] if old < len(weights) else "elbow" for old in used]
    return [places[old] for old in used], flat, parts


def shade(gun_places, gun_faces, shells):
    """Gives every face of the hands its share of the open sky: how much of what it faces, within
    a few centimetres, is free of the gun and the hands. Faces between the fingers and against
    the gun come out darker, which is what tells the fingers apart at the size they are seen."""
    places = [tuple(place) for place in gun_places]
    faces = list(gun_faces)
    for shell_places, shell_faces, _ in shells:
        base = len(places)
        places += [tuple(place) for place in shell_places]
        faces += [tuple(base + corner for corner in corners) for _, corners in shell_faces]
    scene = BVHTree.FromPolygons(places, faces)
    # Directions spread evenly over a hemisphere about +Z, more of them near its pole.
    rays = []
    for i in range(SKY_RAYS):
        height = math.sqrt((i + 0.5) / SKY_RAYS)
        around = i * 2.399963
        ring = math.sqrt(1.0 - height * height)
        rays.append(Vector((ring * math.cos(around), ring * math.sin(around), height)))
    shaded = []
    for shell_places, shell_faces, parts in shells:
        lit = []
        for finish, corners in shell_faces:
            corner_places = [shell_places[corner] for corner in corners]
            middle = sum(corner_places, Vector()) / len(corners)
            normal = sum((a.cross(b) for a, b in zip(corner_places, corner_places[1:] + corner_places[:1])), Vector()).normalized()
            turn = Vector((0.0, 0.0, 1.0)).rotation_difference(normal).to_matrix()
            start = middle + normal * 0.0005
            # A sleeve is cloth over one round arm: shading it by what is near only blotches it.
            free = 1.0 if finish == "sleeve" else sum(scene.ray_cast(start, turn @ ray, SKY_REACH)[0] is None for ray in rays) / SKY_RAYS
            lit.append((f"{finish}.{round((DARKEST + (1.0 - DARKEST) * free) * 20) * 5}", corners))
        shaded.append((shell_places, lit, parts))
    return shaded


def write_obj(path, shells):
    lines = [
        "# Written by tools/hands/pose.py: WRAD ARMS by wriks (CC0), posed on the gun.",
        "# A face's material is its finish and, after the dot, the percent of the finish's color it keeps.",
    ]
    base = 1
    for places, faces in shells:
        lines += [f"v {place.x:.5f} {place.y:.5f} {place.z:.5f}" for place in places]
        finish = None
        for name, corners in sorted(faces, key=lambda face: face[0]):
            if name != finish:
                lines.append(f"usemtl {name}")
                finish = name
            lines.append("f " + " ".join(str(base + corner) for corner in corners))
        base += len(places)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", newline="\n")


FINISHES = {"glove": 0x4C515C, "pad": 0x70757F, "sleeve": 0x2A2F39}


def linear(hex_color):
    channel = lambda value: ((value / 255 + 0.055) / 1.055) ** 2.4 if value > 10 else value / 255 / 12.92
    return tuple(channel((hex_color >> shift) & 255) for shift in (16, 8, 0))


def show(name, places, faces, colors):
    """A mesh object for the pictures, colored per face corner."""
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([tuple(place) for place in places], [], [tuple(corners) for corners in faces])
    # The finishes as they are, and lightened for the close-ups.
    for layer, gain in (("finish", 2.2), ("light", 4.0)):
        layer = mesh.color_attributes.new(layer, "FLOAT_COLOR", "CORNER")
        for polygon in mesh.polygons:
            for loop in polygon.loop_indices:
                color = colors(polygon.index, mesh.loops[loop].vertex_index)
                layer.data[loop].color = (*(min(1.0, channel * gain) for channel in color), 1.0)
    shown = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(shown)
    return shown


def look(directory, name, eye, held, hands, shown):
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "VERTEX"
    scene.display.shading.show_cavity = False
    scene.world.color = (0.012, 0.014, 0.018) if scene.world else (0, 0, 0)
    skin.hide_render = True
    camera = bpy.data.objects["Camera"]
    scene.camera = camera
    camera.data.clip_start, camera.data.clip_end = 0.1, 50.0
    # As the player sees it: the eye is the origin of the space the gun is held in.
    camera.data.type = "PERSP"
    camera.data.sensor_fit = "VERTICAL"
    camera.data.angle_y = 2.0 * math.atan(held["tan_v"])
    camera.matrix_world = eye.inverted()
    scene.render.resolution_x, scene.render.resolution_y = 2560, 1440
    scene.render.filepath = str(directory / f"{name}_eye.png")
    for item in shown:
        item.data.color_attributes.active_color = item.data.color_attributes["finish"]
    bpy.ops.render.render(write_still=True)
    for item in shown:
        item.data.color_attributes.active_color = item.data.color_attributes["light"]
    # Close-ups around each hand, from six sides.
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = 0.26
    camera.data.clip_start, camera.data.clip_end = 0.01, 10.0
    scene.render.resolution_x, scene.render.resolution_y = 800, 800
    views = {"left": (-1, 0.15, 0.1), "right": (1, 0.15, 0.1), "top": (0.1, 1, 0.3), "under": (0.1, -1, 0.2), "back": (-0.35, 0.35, 1), "front": (-0.3, 0.2, -1)}
    for hand in hands:
        centre = arms.matrix_world @ hand.bone("finger_middle1").head
        for view, direction in views.items():
            direction = Vector(direction).normalized()
            right = Vector((0, 1, 0)).cross(direction).normalized()
            up = direction.cross(right)
            camera.matrix_world = frame(centre + direction, right, up, direction)
            scene.render.filepath = str(directory / f"{name}_{hand.side}_{view}.png")
            bpy.ops.render.render(write_still=True)


def main():
    arguments = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    directory = Path(arguments[arguments.index("--look") + 1]) if "--look" in arguments else None
    prepare()
    for name, holds in HOLDS.items():
        places, colors, faces, eye, held = read_gun(name)
        gun = BVHTree.FromPolygons([tuple(place) for place in places], faces)
        hands, shells = [], []
        for hold in holds:
            hand = Hand(hold, gun, eye)
            for finger in FINGERS:
                if finger in hold:
                    print(name, hold["side"], finger, "turns and millimetres off", *hand.reach(finger, hold[finger]))
                else:
                    print(name, hold["side"], finger, "bends", [round(bend) for bend in hand.close(finger)])
            print(name, hold["side"], "thumb turns and millimetres off", *hand.reach("thumb", hold["thumb"]))
            hands.append(hand)
            shells.append(shell(hold["side"]))
        shells = shade(places, faces, shells)
        write_obj(MODELS / f"{name}_hands.obj", [shell[:2] for shell in shells])
        # What of the hands is sunk into the gun, by the bone it follows: corners and the deepest (mm).
        for hold, (shell_places, _, parts) in zip(holds, shells):
            sunk = {}
            for place, part in zip(shell_places, parts):
                if inside(gun, place):
                    count, deepest = sunk.get(part, (0, 0.0))
                    sunk[part] = (count + 1, max(deepest, round(gun.find_nearest(place)[3] * 1000, 1)))
            print(name, hold["side"], "sunk into the gun:", sunk)
        if len(shells) == 2:
            gap = min((first - second).length for first in shells[0][0] for second in shells[1][0])
            print(name, f"the arms come within {gap * 1000:.0f} mm of each other")
        print(name, "corners", sum(len(shell[0]) for shell in shells), "faces", sum(len(shell[1]) for shell in shells))
        # Nothing of an arm may cross the near plane inside the view: it would be seen cut open.
        for shell_places, _, _ in shells:
            for place in shell_places:
                seen = eye @ place
                if seen.z > -0.13:
                    in_view = abs(seen.y) < -seen.z * held["tan_v"] and abs(seen.x) < -seen.z * held["tan_v"] * held["aspect"]
                    assert seen.z >= 0.0 or not in_view, f"{name}: an arm reaches the eye inside the view at {tuple(seen)}"
        if directory:
            directory.mkdir(parents=True, exist_ok=True)
            shown = [show(f"{name}_gun", places, faces, lambda _, vertex: colors[vertex])]
            for shell_places, shell_faces, _ in shells:
                tones = [tuple(channel * int(finish.split(".")[1]) / 100 for channel in linear(FINISHES[finish.split(".")[0]])) for finish, _ in shell_faces]
                shown.append(show(f"{name}_hand", shell_places, [corners for _, corners in shell_faces], lambda face, _: tones[face]))
            look(directory, name, eye, held, hands, shown)
            for item in shown:
                bpy.data.objects.remove(item)
        # Back to rest for the next gun.
        for bone in pose:
            bone.matrix_basis = Matrix.Identity(4)
        update()


main()
