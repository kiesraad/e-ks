use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Type, parse_macro_input};

/// Derive form validation methods with field annotations.
///
/// Generates three inherent methods on the form struct:
/// - `validate_into(self, base: &Target)` parses the form into a copy of
///   `base`, replacing the mapped fields and collecting all errors.
/// - `validate_create(self)` validates into `Target::default()`.
/// - `validate_update(self, current: &Target)` validates into `current`.
///
/// Supported annotations:
/// - `#[validate(target = "Type")]` on the struct.
/// - `#[validate(parse = "Type")]` to parse via `Type::from_str`.
/// - `#[validate(optional)]` to treat empty strings as `None` (requires `parse`).
/// - `#[validate(not_empty)]` to reject empty values via `is_empty`.
/// - `#[validate(ignore)]` to skip validation and mapping for a field.
/// - `#[validate(flatten)]` to validate a nested form and prefix its errors with `field.child`.
#[proc_macro_derive(Validate, attributes(validate))]
pub fn derive_validate(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_validate(&input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Default)]
struct FieldOptions {
    optional: bool,
    ignore: bool,
    parse_ty: Option<Type>,
    flatten: bool,
    not_empty: bool,
}

fn expand_validate(input: &DeriveInput) -> syn::Result<TokenStream> {
    let target = parse_struct_options(input)?;
    let struct_name = &input.ident;

    let fields = collect_named_fields(input)?;
    let field_blocks = build_field_blocks(&fields)?;
    let tokens = build_validate_impl(struct_name, &target, &field_blocks);

    Ok(tokens.into())
}

struct FieldBlocks {
    field_inits: Vec<proc_macro2::TokenStream>,
    field_blocks: Vec<proc_macro2::TokenStream>,
}

fn collect_named_fields(input: &DeriveInput) -> syn::Result<Vec<&syn::Field>> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(fields.named.iter().collect()),
            _ => Err(syn::Error::new_spanned(
                &data.fields,
                "Validate can only be derived for structs with named fields",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            input,
            "Validate can only be derived for structs",
        )),
    }
}

fn build_field_blocks(fields: &[&syn::Field]) -> syn::Result<FieldBlocks> {
    let mut field_inits = Vec::new();
    let mut field_blocks = Vec::new();

    for field in fields {
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "expected named field"))?;
        let field_name = ident.to_string();
        let opts = parse_field_options(field)?;

        if opts.ignore {
            continue;
        }

        if opts.not_empty {
            let error = if is_type_named(&field.ty, "Vec") || is_type_named(&field.ty, "BTreeSet") {
                quote!(crate::form::ValidationError::ChooseAtLeastOneOption)
            } else {
                quote!(crate::form::ValidationError::ValueShouldNotBeEmpty)
            };
            field_blocks.push(quote! {
                if self.#ident.is_empty() {
                    errors.push((#field_name.to_string(), #error));
                }
            });
        }

        let validation = build_field_validation(ident, &field_name, &field.ty, &opts);
        let expr = &validation.expr;
        field_blocks.push(quote! {
            let #ident = #expr;
        });
        if validation.validated {
            field_inits.push(quote! {
                #ident: #ident.expect("validated field")
            });
        } else {
            field_inits.push(quote! {
                #ident
            });
        }
    }

    Ok(FieldBlocks {
        field_inits,
        field_blocks,
    })
}

fn is_type_named(ty: &Type, name: &str) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

fn build_validate_impl(
    struct_name: &syn::Ident,
    target: &Type,
    field_blocks: &FieldBlocks,
) -> proc_macro2::TokenStream {
    let FieldBlocks {
        field_inits,
        field_blocks,
    } = field_blocks;

    quote! {
        impl #struct_name {
            /// Validate into a new target, filling unmapped fields with defaults.
            pub fn validate_create(self) -> Result<#target, crate::form::FormData<Self>> {
                self.validate_into(&<#target as ::std::default::Default>::default())
            }

            /// Validate into a copy of `current`, replacing the mapped fields.
            pub fn validate_update(
                self,
                current: &#target,
            ) -> Result<#target, crate::form::FormData<Self>> {
                self.validate_into(current)
            }

            /// Validate into a copy of `base`, collecting all field errors.
            pub fn validate_into(
                self,
                base: &#target,
            ) -> Result<#target, crate::form::FormData<Self>> {
                let mut errors: crate::form::FieldErrors = Vec::new();

                #(#field_blocks)*

                if !errors.is_empty() {
                    tracing::debug!("Validation errors: {errors:?}");
                    return Err(crate::form::FormData::new_with_errors(self, errors));
                }

                #[allow(clippy::needless_update)]
                Ok(#target {
                    #(#field_inits,)*
                    ..base.clone()
                })
            }
        }
    }
}

/// Parse the `#[validate(target = "Type")]` attribute from a struct.
fn parse_struct_options(input: &DeriveInput) -> syn::Result<Type> {
    let mut target = None;

    for attr in &input.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("target") {
                let lit: LitStr = meta.value()?.parse()?;
                target = Some(lit.parse::<Type>()?);
                return Ok(());
            }

            Err(meta.error("unsupported validate attribute on struct"))
        })?;
    }

    target.ok_or_else(|| {
        syn::Error::new_spanned(input, "missing #[validate(target = \"Type\")] on struct")
    })
}

/// Parse field-level `#[validate(...)]` options and check their combination.
fn parse_field_options(field: &syn::Field) -> syn::Result<FieldOptions> {
    let mut opts = FieldOptions::default();

    for attr in &field.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("optional") {
                opts.optional = true;
            } else if meta.path.is_ident("not_empty") {
                opts.not_empty = true;
            } else if meta.path.is_ident("ignore") {
                opts.ignore = true;
            } else if meta.path.is_ident("flatten") {
                opts.flatten = true;
            } else if meta.path.is_ident("parse") {
                let lit: LitStr = meta.value()?.parse()?;
                opts.parse_ty = Some(lit.parse::<Type>()?);
            } else {
                return Err(meta.error("unsupported validate attribute on field"));
            }
            Ok(())
        })?;
    }

    check_field_option_conflicts(field, &opts)?;

    Ok(opts)
}

fn check_field_option_conflicts(field: &syn::Field, opts: &FieldOptions) -> syn::Result<()> {
    let error = |message| Err(syn::Error::new_spanned(field, message));
    let other_count =
        opts.optional as u8 + opts.not_empty as u8 + u8::from(opts.parse_ty.is_some());

    if opts.ignore && (opts.flatten || other_count > 0) {
        return error("ignore cannot be combined with other validate options");
    }
    if opts.flatten && other_count > 0 {
        return error("flatten cannot be combined with other validate options");
    }
    if opts.not_empty && (opts.optional || opts.parse_ty.is_some()) {
        return error("not_empty cannot be combined with parse or optional");
    }
    if opts.optional && opts.parse_ty.is_none() {
        return error("optional requires parse");
    }

    Ok(())
}

struct FieldValidation {
    expr: proc_macro2::TokenStream,
    validated: bool,
}

/// Dispatch to the correct validation strategy for a field.
fn build_field_validation(
    ident: &syn::Ident,
    field_name: &str,
    ty: &Type,
    opts: &FieldOptions,
) -> FieldValidation {
    if opts.flatten {
        return build_flatten_validation(ident, field_name, is_type_named(ty, "Option"));
    }

    if let Some(ty) = &opts.parse_ty {
        return build_parse_validation(ident, field_name, ty, opts.optional);
    }

    // Pass-through: fields without a validator are cloned as-is.
    FieldValidation {
        expr: quote!(self.#ident.clone()),
        validated: false,
    }
}

/// Build validation for a nested form (`#[validate(flatten)]`), forwarding and
/// prefixing its errors (`address` field errors become `address.postal_code`).
fn build_flatten_validation(
    ident: &syn::Ident,
    field_name: &str,
    optional: bool,
) -> FieldValidation {
    let extend_errors = quote! {
        errors.extend(form_data.errors().into_iter().map(|(name, err)| {
            (format!("{}.{}", #field_name, name), err)
        }));
    };

    let expr = if optional {
        quote!({
            match self.#ident.clone() {
                Some(value) => {
                    let nested_base = base.#ident.clone().unwrap_or_default();
                    match value.validate_into(&nested_base) {
                        Ok(value) => Some(Some(value)),
                        Err(form_data) => { #extend_errors None }
                    }
                }
                None => Some(None),
            }
        })
    } else {
        quote!({
            match self.#ident.clone().validate_into(&base.#ident) {
                Ok(value) => Some(value),
                Err(form_data) => { #extend_errors None }
            }
        })
    };

    FieldValidation {
        expr,
        validated: true,
    }
}

/// Build validation for `#[validate(parse = "...")]` fields, parsing the
/// trimmed input via `FromStr` (e.g. `first_name: String` into `FirstName`).
fn build_parse_validation(
    ident: &syn::Ident,
    field_name: &str,
    ty: &Type,
    optional: bool,
) -> FieldValidation {
    let if_empty = if optional {
        quote!(Some(None))
    } else {
        quote!(
            errors.push((
                #field_name.to_string(),
                crate::form::ValidationError::ValueShouldNotBeEmpty,
            ));
            None
        )
    };

    let res = if optional {
        quote!(Some(Some(value)))
    } else {
        quote!(Some(value))
    };

    let expr = quote!({
        let value = self.#ident.trim();
        if value.is_empty() {
            #if_empty
        } else {
            match <#ty as ::std::str::FromStr>::from_str(value) {
                Ok(value) => #res,
                Err(err) => {
                    errors.push((
                        #field_name.to_string(),
                        crate::form::IntoValidationError::into_validation_error(err)
                    ));
                    None
                }
            }
        }
    });

    FieldValidation {
        expr,
        validated: true,
    }
}
