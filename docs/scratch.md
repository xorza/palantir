- gradients, textures
- showcase agent testing


    #[serde(skip)]
    pub anim: AnimSpec,



skip animations if target ~= current

dedupe pub fn pick(&self, state: ResponseState, focused: bool) -> &WidgetLook {
        if state.disabled {
