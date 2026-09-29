use crate::cssom::CSSStyleDeclaration;
use crate::dom::DOMException;
use crate::js::Types;
use crate::js::bindings::this_as;
use crate::webidl::LegacyPlatformObject;
use crate::webidl::bindings::{
    AttributeDef, BindingFn, InterfaceDefinition, OperationDef, WebIdlInterface,
    create_interface_instance,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for CSSStyleDeclaration {
    const NAME: &'static str = "CSSStyleDeclaration";

    fn value_iterator() -> bool {
        true
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(attribute_def("length", get_length, None));
        def.add_attribute(attribute_def("cssText", get_css_text, Some(set_css_text)));
        for (id, length, method) in [
            ("item", 1, item as BindingFn<Types>),
            ("getPropertyValue", 1, get_property_value),
            ("getPropertyPriority", 1, get_property_priority),
            ("setProperty", 2, set_property),
            ("removeProperty", 1, remove_property),
        ] {
            def.add_operation(OperationDef {
                id,
                length,
                method,
                static_: false,
                unforgeable: false,
                promise_type: false,
                exposed: None,
            });
        }
        for (id, getter, setter) in CAMEL_CASED_ATTRIBUTES
            .iter()
            .chain(DASHED_PROPERTY_CAMEL_CASED_ATTRIBUTES)
            .chain(DASHED_ATTRIBUTES)
            .chain(WEBKIT_CASED_ATTRIBUTES)
        {
            def.add_attribute(attribute_def(id, *getter, Some(*setter)));
        }
    }
}

impl LegacyPlatformObject for CSSStyleDeclaration {
    const SUPPORTS_INDEXED_PROPERTIES: bool = true;
    const SUPPORTS_NAMED_PROPERTIES: bool = false;

    fn supported_property_indices(
        &self,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<u32, Types> {
        Ok(CSSStyleDeclaration::length(self))
    }

    fn determine_the_value_of_an_indexed_property(
        &self,
        index: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        Ok(string_value(&CSSStyleDeclaration::item(self, index), ec))
    }

    fn supported_property_names(
        &self,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<String>, Types> {
        Ok(Vec::new())
    }

    fn determine_the_value_of_a_named_property(
        &self,
        _name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        Ok(ec.value_undefined())
    }
}

fn attribute_def(
    id: &'static str,
    getter: BindingFn<Types>,
    setter: Option<BindingFn<Types>>,
) -> AttributeDef<Types> {
    AttributeDef {
        id,
        getter,
        setter,
        static_: false,
        unforgeable: false,
        promise_type: false,
        legacy_lenient_this: false,
        replaceable: false,
        put_forwards: None,
        legacy_lenient_setter: false,
        exposed: None,
    }
}

fn declaration_block(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<CSSStyleDeclaration, Types> {
    this_as::<CSSStyleDeclaration>(this, "CSSStyleDeclaration", ec)
}

fn string_value(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    ec.value_from_string(ec.js_string_from_str(value))
}

fn string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.get(index).cloned().unwrap_or(undefined))
}

fn legacy_null_to_empty_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    match args.get(index) {
        Some(value) if Types::value_is_null(value) => Ok(String::new()),
        _ => string_argument(args, index, ec),
    }
}

fn dom_exception_value(error: DOMException, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    create_interface_instance::<Types, DOMException>(error, ec)
        .map(Types::value_from_object)
        .unwrap_or_else(|err| err)
}

fn get_length(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let length = declaration_block(this, ec)?.length();
    Ok(ec.value_from_number(length as f64))
}

fn get_css_text(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let css_text = declaration_block(this, ec)?.css_text();
    Ok(string_value(&css_text, ec))
}

fn set_css_text(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = legacy_null_to_empty_string_argument(args, 0, ec)?;
    declaration_block(this, ec)?
        .set_css_text(&value)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let index = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    let name = declaration_block(this, ec)?.item(index);
    Ok(string_value(&name, ec))
}

fn get_property_value(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let property = string_argument(args, 0, ec)?;
    let value = declaration_block(this, ec)?.get_property_value(&property);
    Ok(string_value(&value, ec))
}

fn get_property_priority(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let property = string_argument(args, 0, ec)?;
    let priority = declaration_block(this, ec)?.get_property_priority(&property);
    Ok(string_value(&priority, ec))
}

fn set_property(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let property = string_argument(args, 0, ec)?;
    let value = legacy_null_to_empty_string_argument(args, 1, ec)?;
    let priority = match args.get(2) {
        Some(priority) if !Types::value_is_undefined(priority) => {
            legacy_null_to_empty_string_argument(args, 2, ec)?
        }
        _ => String::new(),
    };
    declaration_block(this, ec)?
        .set_property(&property, &value, &priority)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn remove_property(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let property = string_argument(args, 0, ec)?;
    let value = declaration_block(this, ec)?
        .remove_property(&property)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(string_value(&value, ec))
}

fn camel_cased_attribute_getter(
    this: &JsValue,
    attribute: &str,
    dashed_prefix: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = declaration_block(this, ec)?.camel_cased_attribute(attribute, dashed_prefix);
    Ok(string_value(&value, ec))
}

fn camel_cased_attribute_setter(
    this: &JsValue,
    attribute: &str,
    dashed_prefix: bool,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = legacy_null_to_empty_string_argument(args, 0, ec)?;
    declaration_block(this, ec)?
        .set_camel_cased_attribute(attribute, dashed_prefix, &value)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn dashed_attribute_getter(
    this: &JsValue,
    attribute: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = declaration_block(this, ec)?.dashed_attribute(attribute);
    Ok(string_value(&value, ec))
}

fn dashed_attribute_setter(
    this: &JsValue,
    attribute: &str,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = legacy_null_to_empty_string_argument(args, 0, ec)?;
    declaration_block(this, ec)?
        .set_dashed_attribute(attribute, &value)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

type PropertyAttribute = (&'static str, BindingFn<Types>, BindingFn<Types>);

macro_rules! camel_cased_attributes {
    ($table:ident, $dashed_prefix:literal; $( ($attribute:literal, $getter:ident, $setter:ident) ),* $(,)?) => {
        $(
            fn $getter(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                camel_cased_attribute_getter(this, $attribute, $dashed_prefix, ec)
            }

            fn $setter(
                this: &JsValue,
                args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                camel_cased_attribute_setter(this, $attribute, $dashed_prefix, args, ec)
            }
        )*

        const $table: &[PropertyAttribute] = &[$( ($attribute, $getter, $setter) ),*];
    };
}

macro_rules! dashed_property_attributes {
    ($camel_table:ident, $dashed_table:ident; $( ($property:literal, $attribute:literal, $getter:ident, $setter:ident, $dashed_getter:ident, $dashed_setter:ident) ),* $(,)?) => {
        $(
            fn $getter(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                camel_cased_attribute_getter(this, $attribute, false, ec)
            }

            fn $setter(
                this: &JsValue,
                args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                camel_cased_attribute_setter(this, $attribute, false, args, ec)
            }

            fn $dashed_getter(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                dashed_attribute_getter(this, $property, ec)
            }

            fn $dashed_setter(
                this: &JsValue,
                args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                dashed_attribute_setter(this, $property, args, ec)
            }
        )*

        const $camel_table: &[PropertyAttribute] = &[$( ($attribute, $getter, $setter) ),*];
        const $dashed_table: &[PropertyAttribute] = &[$( ($property, $dashed_getter, $dashed_setter) ),*];
    };
}

// The supported CSS properties are stylo's properties enabled for all
// content; the tables list each one under its camel-cased attribute, its
// dashed attribute when its name contains "-", and its webkit-cased
// attribute when its name begins with "-webkit-".
camel_cased_attributes!(CAMEL_CASED_ATTRIBUTES, false;
    ("animation", get_animation, set_animation),
    ("background", get_background, set_background),
    ("border", get_border, set_border),
    ("bottom", get_bottom, set_bottom),
    ("clear", get_clear, set_clear),
    ("clip", get_clip, set_clip),
    ("color", get_color, set_color),
    ("columns", get_columns, set_columns),
    ("contain", get_contain, set_contain),
    ("content", get_content, set_content),
    ("cursor", get_cursor, set_cursor),
    ("direction", get_direction, set_direction),
    ("display", get_display, set_display),
    ("filter", get_filter, set_filter),
    ("flex", get_flex, set_flex),
    ("float", get_float, set_float),
    ("font", get_font, set_font),
    ("gap", get_gap, set_gap),
    ("grid", get_grid, set_grid),
    ("height", get_height, set_height),
    ("inset", get_inset, set_inset),
    ("isolation", get_isolation, set_isolation),
    ("left", get_left, set_left),
    ("margin", get_margin, set_margin),
    ("opacity", get_opacity, set_opacity),
    ("order", get_order, set_order),
    ("outline", get_outline, set_outline),
    ("overflow", get_overflow, set_overflow),
    ("padding", get_padding, set_padding),
    ("perspective", get_perspective, set_perspective),
    ("position", get_position, set_position),
    ("quotes", get_quotes, set_quotes),
    ("right", get_right, set_right),
    ("rotate", get_rotate, set_rotate),
    ("scale", get_scale, set_scale),
    ("top", get_top, set_top),
    ("transform", get_transform, set_transform),
    ("transition", get_transition, set_transition),
    ("translate", get_translate, set_translate),
    ("visibility", get_visibility, set_visibility),
    ("width", get_width, set_width),
);

camel_cased_attributes!(WEBKIT_CASED_ATTRIBUTES, true;
    ("webkitPerspective", get_webkit_cased_webkit_perspective, set_webkit_cased_webkit_perspective),
    ("webkitTextSecurity", get_webkit_cased_webkit_text_security, set_webkit_cased_webkit_text_security),
    ("webkitTransform", get_webkit_cased_webkit_transform, set_webkit_cased_webkit_transform),
);

dashed_property_attributes!(DASHED_PROPERTY_CAMEL_CASED_ATTRIBUTES, DASHED_ATTRIBUTES;
    ("-webkit-perspective", "WebkitPerspective", get_webkit_perspective, set_webkit_perspective, get_dashed_webkit_perspective, set_dashed_webkit_perspective),
    ("-webkit-text-security", "WebkitTextSecurity", get_webkit_text_security, set_webkit_text_security, get_dashed_webkit_text_security, set_dashed_webkit_text_security),
    ("-webkit-transform", "WebkitTransform", get_webkit_transform, set_webkit_transform, get_dashed_webkit_transform, set_dashed_webkit_transform),
    ("align-content", "alignContent", get_align_content, set_align_content, get_dashed_align_content, set_dashed_align_content),
    ("align-items", "alignItems", get_align_items, set_align_items, get_dashed_align_items, set_dashed_align_items),
    ("align-self", "alignSelf", get_align_self, set_align_self, get_dashed_align_self, set_dashed_align_self),
    ("alignment-baseline", "alignmentBaseline", get_alignment_baseline, set_alignment_baseline, get_dashed_alignment_baseline, set_dashed_alignment_baseline),
    ("animation-composition", "animationComposition", get_animation_composition, set_animation_composition, get_dashed_animation_composition, set_dashed_animation_composition),
    ("animation-delay", "animationDelay", get_animation_delay, set_animation_delay, get_dashed_animation_delay, set_dashed_animation_delay),
    ("animation-direction", "animationDirection", get_animation_direction, set_animation_direction, get_dashed_animation_direction, set_dashed_animation_direction),
    ("animation-duration", "animationDuration", get_animation_duration, set_animation_duration, get_dashed_animation_duration, set_dashed_animation_duration),
    ("animation-fill-mode", "animationFillMode", get_animation_fill_mode, set_animation_fill_mode, get_dashed_animation_fill_mode, set_dashed_animation_fill_mode),
    ("animation-iteration-count", "animationIterationCount", get_animation_iteration_count, set_animation_iteration_count, get_dashed_animation_iteration_count, set_dashed_animation_iteration_count),
    ("animation-name", "animationName", get_animation_name, set_animation_name, get_dashed_animation_name, set_dashed_animation_name),
    ("animation-play-state", "animationPlayState", get_animation_play_state, set_animation_play_state, get_dashed_animation_play_state, set_dashed_animation_play_state),
    ("animation-range-end", "animationRangeEnd", get_animation_range_end, set_animation_range_end, get_dashed_animation_range_end, set_dashed_animation_range_end),
    ("animation-range-start", "animationRangeStart", get_animation_range_start, set_animation_range_start, get_dashed_animation_range_start, set_dashed_animation_range_start),
    ("animation-timeline", "animationTimeline", get_animation_timeline, set_animation_timeline, get_dashed_animation_timeline, set_dashed_animation_timeline),
    ("animation-timing-function", "animationTimingFunction", get_animation_timing_function, set_animation_timing_function, get_dashed_animation_timing_function, set_dashed_animation_timing_function),
    ("aspect-ratio", "aspectRatio", get_aspect_ratio, set_aspect_ratio, get_dashed_aspect_ratio, set_dashed_aspect_ratio),
    ("backdrop-filter", "backdropFilter", get_backdrop_filter, set_backdrop_filter, get_dashed_backdrop_filter, set_dashed_backdrop_filter),
    ("backface-visibility", "backfaceVisibility", get_backface_visibility, set_backface_visibility, get_dashed_backface_visibility, set_dashed_backface_visibility),
    ("background-attachment", "backgroundAttachment", get_background_attachment, set_background_attachment, get_dashed_background_attachment, set_dashed_background_attachment),
    ("background-blend-mode", "backgroundBlendMode", get_background_blend_mode, set_background_blend_mode, get_dashed_background_blend_mode, set_dashed_background_blend_mode),
    ("background-clip", "backgroundClip", get_background_clip, set_background_clip, get_dashed_background_clip, set_dashed_background_clip),
    ("background-color", "backgroundColor", get_background_color, set_background_color, get_dashed_background_color, set_dashed_background_color),
    ("background-image", "backgroundImage", get_background_image, set_background_image, get_dashed_background_image, set_dashed_background_image),
    ("background-origin", "backgroundOrigin", get_background_origin, set_background_origin, get_dashed_background_origin, set_dashed_background_origin),
    ("background-position", "backgroundPosition", get_background_position, set_background_position, get_dashed_background_position, set_dashed_background_position),
    ("background-position-x", "backgroundPositionX", get_background_position_x, set_background_position_x, get_dashed_background_position_x, set_dashed_background_position_x),
    ("background-position-y", "backgroundPositionY", get_background_position_y, set_background_position_y, get_dashed_background_position_y, set_dashed_background_position_y),
    ("background-repeat", "backgroundRepeat", get_background_repeat, set_background_repeat, get_dashed_background_repeat, set_dashed_background_repeat),
    ("background-size", "backgroundSize", get_background_size, set_background_size, get_dashed_background_size, set_dashed_background_size),
    ("baseline-shift", "baselineShift", get_baseline_shift, set_baseline_shift, get_dashed_baseline_shift, set_dashed_baseline_shift),
    ("baseline-source", "baselineSource", get_baseline_source, set_baseline_source, get_dashed_baseline_source, set_dashed_baseline_source),
    ("block-size", "blockSize", get_block_size, set_block_size, get_dashed_block_size, set_dashed_block_size),
    ("border-block", "borderBlock", get_border_block, set_border_block, get_dashed_border_block, set_dashed_border_block),
    ("border-block-color", "borderBlockColor", get_border_block_color, set_border_block_color, get_dashed_border_block_color, set_dashed_border_block_color),
    ("border-block-end", "borderBlockEnd", get_border_block_end, set_border_block_end, get_dashed_border_block_end, set_dashed_border_block_end),
    ("border-block-end-color", "borderBlockEndColor", get_border_block_end_color, set_border_block_end_color, get_dashed_border_block_end_color, set_dashed_border_block_end_color),
    ("border-block-end-style", "borderBlockEndStyle", get_border_block_end_style, set_border_block_end_style, get_dashed_border_block_end_style, set_dashed_border_block_end_style),
    ("border-block-end-width", "borderBlockEndWidth", get_border_block_end_width, set_border_block_end_width, get_dashed_border_block_end_width, set_dashed_border_block_end_width),
    ("border-block-start", "borderBlockStart", get_border_block_start, set_border_block_start, get_dashed_border_block_start, set_dashed_border_block_start),
    ("border-block-start-color", "borderBlockStartColor", get_border_block_start_color, set_border_block_start_color, get_dashed_border_block_start_color, set_dashed_border_block_start_color),
    ("border-block-start-style", "borderBlockStartStyle", get_border_block_start_style, set_border_block_start_style, get_dashed_border_block_start_style, set_dashed_border_block_start_style),
    ("border-block-start-width", "borderBlockStartWidth", get_border_block_start_width, set_border_block_start_width, get_dashed_border_block_start_width, set_dashed_border_block_start_width),
    ("border-block-style", "borderBlockStyle", get_border_block_style, set_border_block_style, get_dashed_border_block_style, set_dashed_border_block_style),
    ("border-block-width", "borderBlockWidth", get_border_block_width, set_border_block_width, get_dashed_border_block_width, set_dashed_border_block_width),
    ("border-bottom", "borderBottom", get_border_bottom, set_border_bottom, get_dashed_border_bottom, set_dashed_border_bottom),
    ("border-bottom-color", "borderBottomColor", get_border_bottom_color, set_border_bottom_color, get_dashed_border_bottom_color, set_dashed_border_bottom_color),
    ("border-bottom-left-radius", "borderBottomLeftRadius", get_border_bottom_left_radius, set_border_bottom_left_radius, get_dashed_border_bottom_left_radius, set_dashed_border_bottom_left_radius),
    ("border-bottom-right-radius", "borderBottomRightRadius", get_border_bottom_right_radius, set_border_bottom_right_radius, get_dashed_border_bottom_right_radius, set_dashed_border_bottom_right_radius),
    ("border-bottom-style", "borderBottomStyle", get_border_bottom_style, set_border_bottom_style, get_dashed_border_bottom_style, set_dashed_border_bottom_style),
    ("border-bottom-width", "borderBottomWidth", get_border_bottom_width, set_border_bottom_width, get_dashed_border_bottom_width, set_dashed_border_bottom_width),
    ("border-collapse", "borderCollapse", get_border_collapse, set_border_collapse, get_dashed_border_collapse, set_dashed_border_collapse),
    ("border-color", "borderColor", get_border_color, set_border_color, get_dashed_border_color, set_dashed_border_color),
    ("border-end-end-radius", "borderEndEndRadius", get_border_end_end_radius, set_border_end_end_radius, get_dashed_border_end_end_radius, set_dashed_border_end_end_radius),
    ("border-end-start-radius", "borderEndStartRadius", get_border_end_start_radius, set_border_end_start_radius, get_dashed_border_end_start_radius, set_dashed_border_end_start_radius),
    ("border-image", "borderImage", get_border_image, set_border_image, get_dashed_border_image, set_dashed_border_image),
    ("border-image-outset", "borderImageOutset", get_border_image_outset, set_border_image_outset, get_dashed_border_image_outset, set_dashed_border_image_outset),
    ("border-image-repeat", "borderImageRepeat", get_border_image_repeat, set_border_image_repeat, get_dashed_border_image_repeat, set_dashed_border_image_repeat),
    ("border-image-slice", "borderImageSlice", get_border_image_slice, set_border_image_slice, get_dashed_border_image_slice, set_dashed_border_image_slice),
    ("border-image-source", "borderImageSource", get_border_image_source, set_border_image_source, get_dashed_border_image_source, set_dashed_border_image_source),
    ("border-image-width", "borderImageWidth", get_border_image_width, set_border_image_width, get_dashed_border_image_width, set_dashed_border_image_width),
    ("border-inline", "borderInline", get_border_inline, set_border_inline, get_dashed_border_inline, set_dashed_border_inline),
    ("border-inline-color", "borderInlineColor", get_border_inline_color, set_border_inline_color, get_dashed_border_inline_color, set_dashed_border_inline_color),
    ("border-inline-end", "borderInlineEnd", get_border_inline_end, set_border_inline_end, get_dashed_border_inline_end, set_dashed_border_inline_end),
    ("border-inline-end-color", "borderInlineEndColor", get_border_inline_end_color, set_border_inline_end_color, get_dashed_border_inline_end_color, set_dashed_border_inline_end_color),
    ("border-inline-end-style", "borderInlineEndStyle", get_border_inline_end_style, set_border_inline_end_style, get_dashed_border_inline_end_style, set_dashed_border_inline_end_style),
    ("border-inline-end-width", "borderInlineEndWidth", get_border_inline_end_width, set_border_inline_end_width, get_dashed_border_inline_end_width, set_dashed_border_inline_end_width),
    ("border-inline-start", "borderInlineStart", get_border_inline_start, set_border_inline_start, get_dashed_border_inline_start, set_dashed_border_inline_start),
    ("border-inline-start-color", "borderInlineStartColor", get_border_inline_start_color, set_border_inline_start_color, get_dashed_border_inline_start_color, set_dashed_border_inline_start_color),
    ("border-inline-start-style", "borderInlineStartStyle", get_border_inline_start_style, set_border_inline_start_style, get_dashed_border_inline_start_style, set_dashed_border_inline_start_style),
    ("border-inline-start-width", "borderInlineStartWidth", get_border_inline_start_width, set_border_inline_start_width, get_dashed_border_inline_start_width, set_dashed_border_inline_start_width),
    ("border-inline-style", "borderInlineStyle", get_border_inline_style, set_border_inline_style, get_dashed_border_inline_style, set_dashed_border_inline_style),
    ("border-inline-width", "borderInlineWidth", get_border_inline_width, set_border_inline_width, get_dashed_border_inline_width, set_dashed_border_inline_width),
    ("border-left", "borderLeft", get_border_left, set_border_left, get_dashed_border_left, set_dashed_border_left),
    ("border-left-color", "borderLeftColor", get_border_left_color, set_border_left_color, get_dashed_border_left_color, set_dashed_border_left_color),
    ("border-left-style", "borderLeftStyle", get_border_left_style, set_border_left_style, get_dashed_border_left_style, set_dashed_border_left_style),
    ("border-left-width", "borderLeftWidth", get_border_left_width, set_border_left_width, get_dashed_border_left_width, set_dashed_border_left_width),
    ("border-radius", "borderRadius", get_border_radius, set_border_radius, get_dashed_border_radius, set_dashed_border_radius),
    ("border-right", "borderRight", get_border_right, set_border_right, get_dashed_border_right, set_dashed_border_right),
    ("border-right-color", "borderRightColor", get_border_right_color, set_border_right_color, get_dashed_border_right_color, set_dashed_border_right_color),
    ("border-right-style", "borderRightStyle", get_border_right_style, set_border_right_style, get_dashed_border_right_style, set_dashed_border_right_style),
    ("border-right-width", "borderRightWidth", get_border_right_width, set_border_right_width, get_dashed_border_right_width, set_dashed_border_right_width),
    ("border-spacing", "borderSpacing", get_border_spacing, set_border_spacing, get_dashed_border_spacing, set_dashed_border_spacing),
    ("border-start-end-radius", "borderStartEndRadius", get_border_start_end_radius, set_border_start_end_radius, get_dashed_border_start_end_radius, set_dashed_border_start_end_radius),
    ("border-start-start-radius", "borderStartStartRadius", get_border_start_start_radius, set_border_start_start_radius, get_dashed_border_start_start_radius, set_dashed_border_start_start_radius),
    ("border-style", "borderStyle", get_border_style, set_border_style, get_dashed_border_style, set_dashed_border_style),
    ("border-top", "borderTop", get_border_top, set_border_top, get_dashed_border_top, set_dashed_border_top),
    ("border-top-color", "borderTopColor", get_border_top_color, set_border_top_color, get_dashed_border_top_color, set_dashed_border_top_color),
    ("border-top-left-radius", "borderTopLeftRadius", get_border_top_left_radius, set_border_top_left_radius, get_dashed_border_top_left_radius, set_dashed_border_top_left_radius),
    ("border-top-right-radius", "borderTopRightRadius", get_border_top_right_radius, set_border_top_right_radius, get_dashed_border_top_right_radius, set_dashed_border_top_right_radius),
    ("border-top-style", "borderTopStyle", get_border_top_style, set_border_top_style, get_dashed_border_top_style, set_dashed_border_top_style),
    ("border-top-width", "borderTopWidth", get_border_top_width, set_border_top_width, get_dashed_border_top_width, set_dashed_border_top_width),
    ("border-width", "borderWidth", get_border_width, set_border_width, get_dashed_border_width, set_dashed_border_width),
    ("box-shadow", "boxShadow", get_box_shadow, set_box_shadow, get_dashed_box_shadow, set_dashed_box_shadow),
    ("box-sizing", "boxSizing", get_box_sizing, set_box_sizing, get_dashed_box_sizing, set_dashed_box_sizing),
    ("caption-side", "captionSide", get_caption_side, set_caption_side, get_dashed_caption_side, set_dashed_caption_side),
    ("caret-color", "caretColor", get_caret_color, set_caret_color, get_dashed_caret_color, set_dashed_caret_color),
    ("clip-path", "clipPath", get_clip_path, set_clip_path, get_dashed_clip_path, set_dashed_clip_path),
    ("color-scheme", "colorScheme", get_color_scheme, set_color_scheme, get_dashed_color_scheme, set_dashed_color_scheme),
    ("column-count", "columnCount", get_column_count, set_column_count, get_dashed_column_count, set_dashed_column_count),
    ("column-gap", "columnGap", get_column_gap, set_column_gap, get_dashed_column_gap, set_dashed_column_gap),
    ("column-span", "columnSpan", get_column_span, set_column_span, get_dashed_column_span, set_dashed_column_span),
    ("column-width", "columnWidth", get_column_width, set_column_width, get_dashed_column_width, set_dashed_column_width),
    ("container-name", "containerName", get_container_name, set_container_name, get_dashed_container_name, set_dashed_container_name),
    ("container-type", "containerType", get_container_type, set_container_type, get_dashed_container_type, set_dashed_container_type),
    ("counter-increment", "counterIncrement", get_counter_increment, set_counter_increment, get_dashed_counter_increment, set_dashed_counter_increment),
    ("counter-reset", "counterReset", get_counter_reset, set_counter_reset, get_dashed_counter_reset, set_dashed_counter_reset),
    ("empty-cells", "emptyCells", get_empty_cells, set_empty_cells, get_dashed_empty_cells, set_dashed_empty_cells),
    ("flex-basis", "flexBasis", get_flex_basis, set_flex_basis, get_dashed_flex_basis, set_dashed_flex_basis),
    ("flex-direction", "flexDirection", get_flex_direction, set_flex_direction, get_dashed_flex_direction, set_dashed_flex_direction),
    ("flex-flow", "flexFlow", get_flex_flow, set_flex_flow, get_dashed_flex_flow, set_dashed_flex_flow),
    ("flex-grow", "flexGrow", get_flex_grow, set_flex_grow, get_dashed_flex_grow, set_dashed_flex_grow),
    ("flex-shrink", "flexShrink", get_flex_shrink, set_flex_shrink, get_dashed_flex_shrink, set_dashed_flex_shrink),
    ("flex-wrap", "flexWrap", get_flex_wrap, set_flex_wrap, get_dashed_flex_wrap, set_dashed_flex_wrap),
    ("font-family", "fontFamily", get_font_family, set_font_family, get_dashed_font_family, set_dashed_font_family),
    ("font-kerning", "fontKerning", get_font_kerning, set_font_kerning, get_dashed_font_kerning, set_dashed_font_kerning),
    ("font-language-override", "fontLanguageOverride", get_font_language_override, set_font_language_override, get_dashed_font_language_override, set_dashed_font_language_override),
    ("font-optical-sizing", "fontOpticalSizing", get_font_optical_sizing, set_font_optical_sizing, get_dashed_font_optical_sizing, set_dashed_font_optical_sizing),
    ("font-size", "fontSize", get_font_size, set_font_size, get_dashed_font_size, set_dashed_font_size),
    ("font-stretch", "fontStretch", get_font_stretch, set_font_stretch, get_dashed_font_stretch, set_dashed_font_stretch),
    ("font-style", "fontStyle", get_font_style, set_font_style, get_dashed_font_style, set_dashed_font_style),
    ("font-synthesis-weight", "fontSynthesisWeight", get_font_synthesis_weight, set_font_synthesis_weight, get_dashed_font_synthesis_weight, set_dashed_font_synthesis_weight),
    ("font-variant", "fontVariant", get_font_variant, set_font_variant, get_dashed_font_variant, set_dashed_font_variant),
    ("font-variant-caps", "fontVariantCaps", get_font_variant_caps, set_font_variant_caps, get_dashed_font_variant_caps, set_dashed_font_variant_caps),
    ("font-variation-settings", "fontVariationSettings", get_font_variation_settings, set_font_variation_settings, get_dashed_font_variation_settings, set_dashed_font_variation_settings),
    ("font-weight", "fontWeight", get_font_weight, set_font_weight, get_dashed_font_weight, set_dashed_font_weight),
    ("grid-area", "gridArea", get_grid_area, set_grid_area, get_dashed_grid_area, set_dashed_grid_area),
    ("grid-auto-columns", "gridAutoColumns", get_grid_auto_columns, set_grid_auto_columns, get_dashed_grid_auto_columns, set_dashed_grid_auto_columns),
    ("grid-auto-flow", "gridAutoFlow", get_grid_auto_flow, set_grid_auto_flow, get_dashed_grid_auto_flow, set_dashed_grid_auto_flow),
    ("grid-auto-rows", "gridAutoRows", get_grid_auto_rows, set_grid_auto_rows, get_dashed_grid_auto_rows, set_dashed_grid_auto_rows),
    ("grid-column", "gridColumn", get_grid_column, set_grid_column, get_dashed_grid_column, set_dashed_grid_column),
    ("grid-column-end", "gridColumnEnd", get_grid_column_end, set_grid_column_end, get_dashed_grid_column_end, set_dashed_grid_column_end),
    ("grid-column-start", "gridColumnStart", get_grid_column_start, set_grid_column_start, get_dashed_grid_column_start, set_dashed_grid_column_start),
    ("grid-row", "gridRow", get_grid_row, set_grid_row, get_dashed_grid_row, set_dashed_grid_row),
    ("grid-row-end", "gridRowEnd", get_grid_row_end, set_grid_row_end, get_dashed_grid_row_end, set_dashed_grid_row_end),
    ("grid-row-start", "gridRowStart", get_grid_row_start, set_grid_row_start, get_dashed_grid_row_start, set_dashed_grid_row_start),
    ("grid-template", "gridTemplate", get_grid_template, set_grid_template, get_dashed_grid_template, set_dashed_grid_template),
    ("grid-template-areas", "gridTemplateAreas", get_grid_template_areas, set_grid_template_areas, get_dashed_grid_template_areas, set_dashed_grid_template_areas),
    ("grid-template-columns", "gridTemplateColumns", get_grid_template_columns, set_grid_template_columns, get_dashed_grid_template_columns, set_dashed_grid_template_columns),
    ("grid-template-rows", "gridTemplateRows", get_grid_template_rows, set_grid_template_rows, get_dashed_grid_template_rows, set_dashed_grid_template_rows),
    ("image-rendering", "imageRendering", get_image_rendering, set_image_rendering, get_dashed_image_rendering, set_dashed_image_rendering),
    ("inline-size", "inlineSize", get_inline_size, set_inline_size, get_dashed_inline_size, set_dashed_inline_size),
    ("inset-block", "insetBlock", get_inset_block, set_inset_block, get_dashed_inset_block, set_dashed_inset_block),
    ("inset-block-end", "insetBlockEnd", get_inset_block_end, set_inset_block_end, get_dashed_inset_block_end, set_dashed_inset_block_end),
    ("inset-block-start", "insetBlockStart", get_inset_block_start, set_inset_block_start, get_dashed_inset_block_start, set_dashed_inset_block_start),
    ("inset-inline", "insetInline", get_inset_inline, set_inset_inline, get_dashed_inset_inline, set_dashed_inset_inline),
    ("inset-inline-end", "insetInlineEnd", get_inset_inline_end, set_inset_inline_end, get_dashed_inset_inline_end, set_dashed_inset_inline_end),
    ("inset-inline-start", "insetInlineStart", get_inset_inline_start, set_inset_inline_start, get_dashed_inset_inline_start, set_dashed_inset_inline_start),
    ("justify-content", "justifyContent", get_justify_content, set_justify_content, get_dashed_justify_content, set_dashed_justify_content),
    ("justify-items", "justifyItems", get_justify_items, set_justify_items, get_dashed_justify_items, set_dashed_justify_items),
    ("justify-self", "justifySelf", get_justify_self, set_justify_self, get_dashed_justify_self, set_dashed_justify_self),
    ("letter-spacing", "letterSpacing", get_letter_spacing, set_letter_spacing, get_dashed_letter_spacing, set_dashed_letter_spacing),
    ("line-break", "lineBreak", get_line_break, set_line_break, get_dashed_line_break, set_dashed_line_break),
    ("line-height", "lineHeight", get_line_height, set_line_height, get_dashed_line_height, set_dashed_line_height),
    ("list-style", "listStyle", get_list_style, set_list_style, get_dashed_list_style, set_dashed_list_style),
    ("list-style-image", "listStyleImage", get_list_style_image, set_list_style_image, get_dashed_list_style_image, set_dashed_list_style_image),
    ("list-style-position", "listStylePosition", get_list_style_position, set_list_style_position, get_dashed_list_style_position, set_dashed_list_style_position),
    ("list-style-type", "listStyleType", get_list_style_type, set_list_style_type, get_dashed_list_style_type, set_dashed_list_style_type),
    ("margin-block", "marginBlock", get_margin_block, set_margin_block, get_dashed_margin_block, set_dashed_margin_block),
    ("margin-block-end", "marginBlockEnd", get_margin_block_end, set_margin_block_end, get_dashed_margin_block_end, set_dashed_margin_block_end),
    ("margin-block-start", "marginBlockStart", get_margin_block_start, set_margin_block_start, get_dashed_margin_block_start, set_dashed_margin_block_start),
    ("margin-bottom", "marginBottom", get_margin_bottom, set_margin_bottom, get_dashed_margin_bottom, set_dashed_margin_bottom),
    ("margin-inline", "marginInline", get_margin_inline, set_margin_inline, get_dashed_margin_inline, set_dashed_margin_inline),
    ("margin-inline-end", "marginInlineEnd", get_margin_inline_end, set_margin_inline_end, get_dashed_margin_inline_end, set_dashed_margin_inline_end),
    ("margin-inline-start", "marginInlineStart", get_margin_inline_start, set_margin_inline_start, get_dashed_margin_inline_start, set_dashed_margin_inline_start),
    ("margin-left", "marginLeft", get_margin_left, set_margin_left, get_dashed_margin_left, set_dashed_margin_left),
    ("margin-right", "marginRight", get_margin_right, set_margin_right, get_dashed_margin_right, set_dashed_margin_right),
    ("margin-top", "marginTop", get_margin_top, set_margin_top, get_dashed_margin_top, set_dashed_margin_top),
    ("mask-image", "maskImage", get_mask_image, set_mask_image, get_dashed_mask_image, set_dashed_mask_image),
    ("max-block-size", "maxBlockSize", get_max_block_size, set_max_block_size, get_dashed_max_block_size, set_dashed_max_block_size),
    ("max-height", "maxHeight", get_max_height, set_max_height, get_dashed_max_height, set_dashed_max_height),
    ("max-inline-size", "maxInlineSize", get_max_inline_size, set_max_inline_size, get_dashed_max_inline_size, set_dashed_max_inline_size),
    ("max-width", "maxWidth", get_max_width, set_max_width, get_dashed_max_width, set_dashed_max_width),
    ("min-block-size", "minBlockSize", get_min_block_size, set_min_block_size, get_dashed_min_block_size, set_dashed_min_block_size),
    ("min-height", "minHeight", get_min_height, set_min_height, get_dashed_min_height, set_dashed_min_height),
    ("min-inline-size", "minInlineSize", get_min_inline_size, set_min_inline_size, get_dashed_min_inline_size, set_dashed_min_inline_size),
    ("min-width", "minWidth", get_min_width, set_min_width, get_dashed_min_width, set_dashed_min_width),
    ("mix-blend-mode", "mixBlendMode", get_mix_blend_mode, set_mix_blend_mode, get_dashed_mix_blend_mode, set_dashed_mix_blend_mode),
    ("object-fit", "objectFit", get_object_fit, set_object_fit, get_dashed_object_fit, set_dashed_object_fit),
    ("object-position", "objectPosition", get_object_position, set_object_position, get_dashed_object_position, set_dashed_object_position),
    ("offset-path", "offsetPath", get_offset_path, set_offset_path, get_dashed_offset_path, set_dashed_offset_path),
    ("outline-color", "outlineColor", get_outline_color, set_outline_color, get_dashed_outline_color, set_dashed_outline_color),
    ("outline-offset", "outlineOffset", get_outline_offset, set_outline_offset, get_dashed_outline_offset, set_dashed_outline_offset),
    ("outline-style", "outlineStyle", get_outline_style, set_outline_style, get_dashed_outline_style, set_dashed_outline_style),
    ("outline-width", "outlineWidth", get_outline_width, set_outline_width, get_dashed_outline_width, set_dashed_outline_width),
    ("overflow-block", "overflowBlock", get_overflow_block, set_overflow_block, get_dashed_overflow_block, set_dashed_overflow_block),
    ("overflow-clip-margin", "overflowClipMargin", get_overflow_clip_margin, set_overflow_clip_margin, get_dashed_overflow_clip_margin, set_dashed_overflow_clip_margin),
    ("overflow-inline", "overflowInline", get_overflow_inline, set_overflow_inline, get_dashed_overflow_inline, set_dashed_overflow_inline),
    ("overflow-wrap", "overflowWrap", get_overflow_wrap, set_overflow_wrap, get_dashed_overflow_wrap, set_dashed_overflow_wrap),
    ("overflow-x", "overflowX", get_overflow_x, set_overflow_x, get_dashed_overflow_x, set_dashed_overflow_x),
    ("overflow-y", "overflowY", get_overflow_y, set_overflow_y, get_dashed_overflow_y, set_dashed_overflow_y),
    ("padding-block", "paddingBlock", get_padding_block, set_padding_block, get_dashed_padding_block, set_dashed_padding_block),
    ("padding-block-end", "paddingBlockEnd", get_padding_block_end, set_padding_block_end, get_dashed_padding_block_end, set_dashed_padding_block_end),
    ("padding-block-start", "paddingBlockStart", get_padding_block_start, set_padding_block_start, get_dashed_padding_block_start, set_dashed_padding_block_start),
    ("padding-bottom", "paddingBottom", get_padding_bottom, set_padding_bottom, get_dashed_padding_bottom, set_dashed_padding_bottom),
    ("padding-inline", "paddingInline", get_padding_inline, set_padding_inline, get_dashed_padding_inline, set_dashed_padding_inline),
    ("padding-inline-end", "paddingInlineEnd", get_padding_inline_end, set_padding_inline_end, get_dashed_padding_inline_end, set_dashed_padding_inline_end),
    ("padding-inline-start", "paddingInlineStart", get_padding_inline_start, set_padding_inline_start, get_dashed_padding_inline_start, set_dashed_padding_inline_start),
    ("padding-left", "paddingLeft", get_padding_left, set_padding_left, get_dashed_padding_left, set_dashed_padding_left),
    ("padding-right", "paddingRight", get_padding_right, set_padding_right, get_dashed_padding_right, set_dashed_padding_right),
    ("padding-top", "paddingTop", get_padding_top, set_padding_top, get_dashed_padding_top, set_dashed_padding_top),
    ("perspective-origin", "perspectiveOrigin", get_perspective_origin, set_perspective_origin, get_dashed_perspective_origin, set_dashed_perspective_origin),
    ("place-content", "placeContent", get_place_content, set_place_content, get_dashed_place_content, set_dashed_place_content),
    ("place-items", "placeItems", get_place_items, set_place_items, get_dashed_place_items, set_dashed_place_items),
    ("place-self", "placeSelf", get_place_self, set_place_self, get_dashed_place_self, set_dashed_place_self),
    ("pointer-events", "pointerEvents", get_pointer_events, set_pointer_events, get_dashed_pointer_events, set_dashed_pointer_events),
    ("position-area", "positionArea", get_position_area, set_position_area, get_dashed_position_area, set_dashed_position_area),
    ("position-try-fallbacks", "positionTryFallbacks", get_position_try_fallbacks, set_position_try_fallbacks, get_dashed_position_try_fallbacks, set_dashed_position_try_fallbacks),
    ("row-gap", "rowGap", get_row_gap, set_row_gap, get_dashed_row_gap, set_dashed_row_gap),
    ("tab-size", "tabSize", get_tab_size, set_tab_size, get_dashed_tab_size, set_dashed_tab_size),
    ("table-layout", "tableLayout", get_table_layout, set_table_layout, get_dashed_table_layout, set_dashed_table_layout),
    ("text-align", "textAlign", get_text_align, set_text_align, get_dashed_text_align, set_dashed_text_align),
    ("text-align-last", "textAlignLast", get_text_align_last, set_text_align_last, get_dashed_text_align_last, set_dashed_text_align_last),
    ("text-decoration", "textDecoration", get_text_decoration, set_text_decoration, get_dashed_text_decoration, set_dashed_text_decoration),
    ("text-decoration-color", "textDecorationColor", get_text_decoration_color, set_text_decoration_color, get_dashed_text_decoration_color, set_dashed_text_decoration_color),
    ("text-decoration-line", "textDecorationLine", get_text_decoration_line, set_text_decoration_line, get_dashed_text_decoration_line, set_dashed_text_decoration_line),
    ("text-decoration-style", "textDecorationStyle", get_text_decoration_style, set_text_decoration_style, get_dashed_text_decoration_style, set_dashed_text_decoration_style),
    ("text-indent", "textIndent", get_text_indent, set_text_indent, get_dashed_text_indent, set_dashed_text_indent),
    ("text-justify", "textJustify", get_text_justify, set_text_justify, get_dashed_text_justify, set_dashed_text_justify),
    ("text-overflow", "textOverflow", get_text_overflow, set_text_overflow, get_dashed_text_overflow, set_dashed_text_overflow),
    ("text-rendering", "textRendering", get_text_rendering, set_text_rendering, get_dashed_text_rendering, set_dashed_text_rendering),
    ("text-shadow", "textShadow", get_text_shadow, set_text_shadow, get_dashed_text_shadow, set_dashed_text_shadow),
    ("text-transform", "textTransform", get_text_transform, set_text_transform, get_dashed_text_transform, set_dashed_text_transform),
    ("text-wrap-mode", "textWrapMode", get_text_wrap_mode, set_text_wrap_mode, get_dashed_text_wrap_mode, set_dashed_text_wrap_mode),
    ("transform-origin", "transformOrigin", get_transform_origin, set_transform_origin, get_dashed_transform_origin, set_dashed_transform_origin),
    ("transform-style", "transformStyle", get_transform_style, set_transform_style, get_dashed_transform_style, set_dashed_transform_style),
    ("transition-behavior", "transitionBehavior", get_transition_behavior, set_transition_behavior, get_dashed_transition_behavior, set_dashed_transition_behavior),
    ("transition-delay", "transitionDelay", get_transition_delay, set_transition_delay, get_dashed_transition_delay, set_dashed_transition_delay),
    ("transition-duration", "transitionDuration", get_transition_duration, set_transition_duration, get_dashed_transition_duration, set_dashed_transition_duration),
    ("transition-property", "transitionProperty", get_transition_property, set_transition_property, get_dashed_transition_property, set_dashed_transition_property),
    ("transition-timing-function", "transitionTimingFunction", get_transition_timing_function, set_transition_timing_function, get_dashed_transition_timing_function, set_dashed_transition_timing_function),
    ("unicode-bidi", "unicodeBidi", get_unicode_bidi, set_unicode_bidi, get_dashed_unicode_bidi, set_dashed_unicode_bidi),
    ("user-select", "userSelect", get_user_select, set_user_select, get_dashed_user_select, set_dashed_user_select),
    ("vertical-align", "verticalAlign", get_vertical_align, set_vertical_align, get_dashed_vertical_align, set_dashed_vertical_align),
    ("white-space", "whiteSpace", get_white_space, set_white_space, get_dashed_white_space, set_dashed_white_space),
    ("white-space-collapse", "whiteSpaceCollapse", get_white_space_collapse, set_white_space_collapse, get_dashed_white_space_collapse, set_dashed_white_space_collapse),
    ("will-change", "willChange", get_will_change, set_will_change, get_dashed_will_change, set_dashed_will_change),
    ("word-break", "wordBreak", get_word_break, set_word_break, get_dashed_word_break, set_dashed_word_break),
    ("word-spacing", "wordSpacing", get_word_spacing, set_word_spacing, get_dashed_word_spacing, set_dashed_word_spacing),
    ("writing-mode", "writingMode", get_writing_mode, set_writing_mode, get_dashed_writing_mode, set_dashed_writing_mode),
    ("z-index", "zIndex", get_z_index, set_z_index, get_dashed_z_index, set_dashed_z_index),
);
